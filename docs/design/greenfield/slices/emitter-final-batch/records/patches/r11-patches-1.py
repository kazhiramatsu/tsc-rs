#!/usr/bin/env python3
"""r11 patch script, part 1 (applied on the r10 bytes): EF7-USING-HOISTED-CLASS-NAME,
EF7-SYSTEM-EXECUTE-ASYNC, EF7-SYSTEM-HOISTED-DEFAULT-EXPORT, EF7-VERBATIM-IMPORT-EXPORT-ELISION,
EF7-ASYNC-ARROW-SUPER-CAPTURE, EF7-ES5-ANONYMOUS-DEFAULT-CLASS-NAME, EF7-STATIC-ACCESSOR-RECEIVER,
EF7-DECORATED-STATIC-FIELD-COMMENT, EF7-SCOPED-NUMBERED-NAMES, plus touched-crate clippy hygiene.
usage: python3 r11-patches-1.py <tree>"""
import sys, os
ROOT = sys.argv[1]
def patch(rel, old, new, count=1):
    path = os.path.join(ROOT, rel)
    s = open(path).read()
    if new in s:
        # `new` may embed `old` (insert-before/after patches), so presence of
        # the complete replacement is the idempotency witness.
        print(f"  already applied: {rel}: {old.strip().splitlines()[0][:60]}")
        return
    n = s.count(old)
    assert n == count, f"{rel}: expected {count} occurrence(s) of anchor, found {n}: {old[:80]!r}"
    s = s.replace(old, new)
    open(path, "w").write(s)
    print(f"  patched {rel}: {old.strip().splitlines()[0][:60]}")

# ---- P1 EF7-USING-HOISTED-CLASS-NAME -------------------------------------------------
patch("crates/emitter/src/builtins/standard_decorators.rs",
"""            SyntaxKind::ClassDeclaration => {
                Ok(Some(DecoratedClassRuntimeName::AnonymousDefaultDeclaration))
            }
""",
"""            SyntaxKind::ClassDeclaration => {
                // EF7-USING-HOISTED-CLASS-NAME: transformESNext's
                // `hoistClassDeclaration` converts a NAMED class declaration
                // into `C = class C {}` (convertToClassExpression keeps the
                // name), so transformESDecorators installs the declared name;
                // only an anonymous (generated `default_N`) declaration is
                // the named-evaluation `"default"` case.
                if let Some(explicit_name_node) = explicit_name_node {
                    if !self.is_generated_binding_name(explicit_name_node)? {
                        return Ok(explicit_name
                            .map(|name| DecoratedClassRuntimeName::Declared(name.to_owned())));
                    }
                }
                Ok(Some(DecoratedClassRuntimeName::AnonymousDefaultDeclaration))
            }
""")

# ---- P2 EF7-SYSTEM-EXECUTE-ASYNC -----------------------------------------------------
patch("crates/emitter/src/builtins/system.rs",
"""        let execute_function = self.create_function_expression(Vec::new(), execute_body, None)?;
""",
"""        // EF7-SYSTEM-EXECUTE-ASYNC: `node.transformFlags & ContainsAwait ?
        // createModifiersFromModifierFlags(Async) : undefined`
        // (createSystemModuleBody, _tsc.js:112209).
        let execute_modifiers = if source_contains_top_level_await(self.context.arena(), root)? {
            let token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::AsyncKeyword,
                TransformFlags::NONE,
            )?;
            Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, vec![token])?
                    .array(),
            )
        } else {
            None
        };
        let execute_function =
            self.create_function_expression(Vec::new(), execute_body, execute_modifiers)?;
""")

patch("crates/emitter/src/builtins/system.rs",
"""#[derive(Clone, Debug)]
struct SystemDependencyGroup {
""",
"""/// `node.transformFlags & ContainsAwait` of the transformed source file:
/// only `createAwaitExpression` sets `ContainsAwait` (_tsc.js:22753 — neither
/// the `await using` declaration list nor the `for await` modifier token
/// does), and the flag stops at function boundaries
/// (`FunctionExcludes`/`ArrowFunctionExcludes`), so it is exactly "an await
/// expression outside every function body" — walked here instead of
/// trusting a root flag word that an earlier pass's `updateSourceFile` may
/// have copied (EF7-SYSTEM-EXECUTE-ASYNC). A lowered `await using` /
/// `for await` contributes through the await expressions it creates.
fn source_contains_top_level_await(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<bool, TransformError> {
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(root.source(), id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(root.source(), id)))?;
        let record = arena.node(node)?;
        match &record.data {
            NodeData::AwaitExpression(_) => return Ok(true),
            NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::ArrowFunction(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::Constructor(_)
            | NodeData::GetAccessor(_)
            | NodeData::SetAccessor(_)
            | NodeData::ClassStaticBlockDeclaration(_) => continue,
            _ => {}
        }
        tsc_syntax::for_each_child(
            &arena.source(root.source())?.syntax().arena,
            record,
            |child| {
                stack.push(child);
                false
            },
        );
    }
    Ok(false)
}

#[derive(Clone, Debug)]
struct SystemDependencyGroup {
""")

# ---- P3 EF7-SYSTEM-HOISTED-DEFAULT-EXPORT --------------------------------------------
patch("crates/emitter/src/builtins/system.rs",
"""        let original = self.context.arena().get_original_node(node);
        if self.context.arena().node(original)?.pos == u32::MAX
            || NodeFlags::from_bits(self.context.arena().node(original)?.flags)
                .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(Vec::new());
        }
        let resolver_node = self.resolver_node(node)?;
        let exported_from_source = self
""",
"""        // EF7-SYSTEM-HOISTED-DEFAULT-EXPORT: `getExports` (_tsc.js:113318)
        // resolves a FileLevel|Optimistic|ReservedInNestedScopes generated
        // identifier (transformESNext's hoisted `_default` binding) through
        // `moduleInfo.exportSpecifiers` instead of the resolver.
        if let Some(metadata) = self.context.arena().metadata(node) {
            if metadata.generated_binding_id().is_some() {
                if !metadata.generated_binding_is_file_level_optimistic()
                    || !metadata.generated_binding_reserved_in_nested_scopes()
                {
                    return Ok(Vec::new());
                }
                return Ok(self
                    .info
                    .common
                    .file_level_generated_binding_exports
                    .get_for_identifier(self.context.arena(), node)
                    .map(<[super::ModuleExportName]>::to_vec)
                    .unwrap_or_default());
            }
        }
        let original = self.context.arena().get_original_node(node);
        if self.context.arena().node(original)?.pos == u32::MAX
            || NodeFlags::from_bits(self.context.arena().node(original)?.flags)
                .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(Vec::new());
        }
        let resolver_node = self.resolver_node(node)?;
        let exported_from_source = self
""")

# ---- P4 EF7-VERBATIM-IMPORT-EXPORT-ELISION -------------------------------------------
patch("crates/emitter/src/builtins.rs",
"""                if retained.is_empty() {
                    return Ok(None);
                }
                let updated = self
                    .context
                    .factory()?
                    .update_node_array(original_array, retained)?;
                data.elements = Some(updated.array());
""",
"""                // EF7-VERBATIM-IMPORT-EXPORT-ELISION: `visitNamedImportBindings`
                // keeps an emptied `{}` under verbatimModuleSyntax
                // (`allowEmpty`, _tsc.js:95548).
                if retained.is_empty() && !self.verbatim_module_syntax {
                    return Ok(None);
                }
                let updated = self
                    .context
                    .factory()?
                    .update_node_array(original_array, retained)?;
                data.elements = Some(updated.array());
""")
patch("crates/emitter/src/builtins.rs",
"""            if !is_type_only
                && self
                    .resolver
                    .is_value_alias_declaration(self.resolver_node(specifier_node)?)?
            {
                retained.push(specifier_node);
            }
        }
        if retained.is_empty() {
            return Ok(None);
        }
        named.elements = Some(
""",
"""            // `visitExportSpecifier`: `!isTypeOnly && (verbatimModuleSyntax ||
            // isValueAliasDeclaration)` (_tsc.js:95599).
            if !is_type_only
                && (self.verbatim_module_syntax
                    || self
                        .resolver
                        .is_value_alias_declaration(self.resolver_node(specifier_node)?)?)
            {
                retained.push(specifier_node);
            }
        }
        // `visitNamedExports(node, allowEmpty = verbatimModuleSyntax)`
        // (_tsc.js:95573, 95589).
        if retained.is_empty() && !self.verbatim_module_syntax {
            return Ok(None);
        }
        named.elements = Some(
""")

# ---- P5 EF7-ASYNC-ARROW-SUPER-CAPTURE -------------------------------------------------
patch("crates/emitter/src/builtins/es2017.rs",
"""            let capture = if is_async {
                Some(self.plan_async_super_capture(original, body)?)
            } else {
                None
            };
            self.super_captures.push(capture);
""",
"""            // EF7-ASYNC-ARROW-SUPER-CAPTURE: `transformMethodBody` plans the
            // `_super` capture for every method/accessor/constructor body,
            // async or not (the checker marks the METHOD when an async arrow
            // inside it references `super`; _tsc.js:101236-101262).
            let capture = if is_async
                || matches!(
                    kind,
                    SyntaxKind::MethodDeclaration
                        | SyntaxKind::GetAccessor
                        | SyntaxKind::SetAccessor
                        | SyntaxKind::Constructor
                )
            {
                Some(self.plan_async_super_capture(original, body)?)
            } else {
                None
            };
            self.super_captures.push(capture);
""")
patch("crates/emitter/src/builtins/es2017.rs",
"""            let modifiers = self.visit_optional_nodes(modifiers);
            let asterisk_token = self.visit_optional_node(asterisk_token);
            let parameters = self.visit_optional_nodes(parameters);
            let body = self.visit_optional_node(body);
            let frame = self.frames.pop();
            debug_assert!(frame.is_some());
            Ok(TransformedFunction {
                modifiers: modifiers?,
                asterisk_token: asterisk_token?,
                parameters: parameters?,
                body: body?,
            })
""",
"""            let modifiers = self.visit_optional_nodes(modifiers);
            let asterisk_token = self.visit_optional_node(asterisk_token);
            let parameters = self.visit_optional_nodes(parameters);
            let body = self.visit_optional_node(body);
            let frame = self.frames.pop();
            debug_assert!(frame.is_some());
            let body = match body {
                Ok(Some(body)) if shape == FunctionShape::Ordinary => self
                    .insert_sync_super_capture_statements(body)
                    .map(Some),
                body => body,
            };
            Ok(TransformedFunction {
                modifiers: modifiers?,
                asterisk_token: asterisk_token?,
                parameters: parameters?,
                body: body?,
            })
""")
patch("crates/emitter/src/builtins/es2017.rs",
"""    fn transform_async_function(
        &mut self,
        original: TransformNode,
        kind: SyntaxKind,
""",
"""    /// `transformMethodBody` (_tsc.js:101236-101262) for a NON-async
    /// method/accessor/constructor: the `_super`/`_superIndex` capture
    /// statements land after the standard prologue when the checker marked
    /// the container (an async arrow inside it references `super`).
    fn insert_sync_super_capture_statements(
        &mut self,
        body: NodeId,
    ) -> Result<NodeId, TransformError> {
        let Some(capture) = self
            .super_captures
            .last()
            .and_then(Option::as_ref)
            .filter(|capture| capture.owns_access)
            .cloned()
        else {
            return Ok(body);
        };
        let has_element_access = capture.has_element_access;
        let statements = self.create_async_super_statements(capture)?;
        if statements.is_empty() {
            return Ok(body);
        }
        let body_node = self.node(body);
        let NodeData::Block(mut data) = self.context.arena().node(body_node)?.data.clone() else {
            return Ok(body);
        };
        let mut existing = self.array_nodes(data.statements)?;
        let mut prologue_end = 0;
        while existing
            .get(prologue_end)
            .is_some_and(|statement| self.is_prologue_statement(*statement))
        {
            prologue_end += 1;
        }
        existing.splice(prologue_end..prologue_end, statements);
        let statements = if let Some(original) = data.statements.map(|array| self.array(array)) {
            self.context
                .factory()?
                .update_node_array(original, existing)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, existing)?
        };
        data.statements = Some(statements.array());
        let flags =
            flags_after_update(self.context.arena(), body_node, &NodeData::Block(data.clone()))?;
        let updated = self
            .context
            .factory()?
            .update_node(body_node, NodeData::Block(data), flags)?;
        // emitBlockFunctionBodyWorker: a body whose emit helpers wrote text
        // (the scoped `_superIndex` helper) prints multi-line even when the
        // source body was single-line.
        let updated = if has_element_access {
            self.context.factory()?.set_multi_line(updated, true)?
        } else {
            updated
        };
        Ok(updated.node())
    }

    fn transform_async_function(
        &mut self,
        original: TransformNode,
        kind: SyntaxKind,
""")

# ---- P6 EF7-ES5-ANONYMOUS-DEFAULT-CLASS-NAME ------------------------------------------
patch("crates/emitter/src/builtins.rs",
"""        let original_modifiers = data.modifiers;
        let mut data = data;
        data.modifiers = self.elide_moved_class_modifiers(data.modifiers)?;
""",
"""        let original_modifiers = data.modifiers;
        let mut data = data;
        data.modifiers = self.elide_moved_class_modifiers(data.modifiers)?;
        // `needsName = moveModifiers && !node.name || …; name = needsName ?
        // node.name ?? factory.getGeneratedNameForNode(node) : node.name`
        // (_tsc.js:94454-94455): an anonymous default class promoted at ES5
        // is named `default_N`.
        if data.name.is_none() {
            let name = self.ensure_generated_declaration_name(original.node(), "default")?;
            data.name = Some(self.create_identifier(&name)?.node());
        }
""")

# ---- P7 EF7-STATIC-ACCESSOR-RECEIVER --------------------------------------------------
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""        let class_alias = environment
            .class_alias
            .clone()
            .expect("downlevel static auto-accessor owns a class constructor binding");
""",
"""        // `tryGetClassThis()`: `lex.classThis ?? lex.classConstructor ??
        // currentClassContainer?.name` (_tsc.js:96252-96255) — a named class
        // without a constructor reference addresses its static storage
        // through its own name.
        let class_alias = match environment.class_alias.clone() {
            Some(class_alias) => class_alias,
            None => ClassBinding::Existing(environment.class_name.clone().expect(
                "downlevel static auto-accessor owns a class name or constructor binding",
            )),
        };
""")
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""    class_alias: Option<ClassBinding>,
    /// `node.emitNode.classThis`: the decorator-supplied class identity.
    class_this: Option<ClassBinding>,
""",
"""    class_alias: Option<ClassBinding>,
    /// `currentClassContainer.name`: the declared class name, the last
    /// `tryGetClassThis()` fallback for static auto-accessor redirectors.
    class_name: Option<String>,
    /// `node.emitNode.classThis`: the decorator-supplied class identity.
    class_this: Option<ClassBinding>,
""")
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""            class_alias: class_alias.clone(),
            class_this: None,
            instance_brand: instance_brand.clone(),
""",
"""            class_alias: class_alias.clone(),
            class_name: class_name.map(str::to_owned),
            class_this: None,
            instance_brand: instance_brand.clone(),
""")
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""        let class_facts = self.scan_class_facts(data.members)?;
""",
"""        let mut class_facts = self.scan_class_facts(data.members)?;
        // getClassFacts (_tsc.js:96960-96962): `isAutoAccessorPropertyDeclaration(member)
        // && shouldTransformAutoAccessors === True && !node.name && !node.emitNode?.classThis`
        // requests a class constructor reference for an anonymous class.
        if data.name.is_none()
            && self.target < ScriptTarget::ES_NEXT
            && self.class_this_binding(original).is_none()
            && self.members_have_static_auto_accessor(data.members)?
        {
            class_facts.has_static_private_or_auto_accessor = true;
        }
""", count=2)
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""    fn class_has_named_evaluation_member(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
""",
"""    fn members_have_static_auto_accessor(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            if let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data {
                if self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                    && self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn class_has_named_evaluation_member(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
""")

# ---- P8 EF7-DECORATED-STATIC-FIELD-COMMENT --------------------------------------------
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""        let statement = self.materialize_private_static_field(operation, true)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_comment_range(crate::CommentRange::new(
                self.source,
                SourceRange::Synthesized,
            ));
        let body = self.create_block(vec![statement], true)?;
""",
"""        let statement = self.materialize_private_static_field(operation, true)?;
        // EF7-DECORATED-STATIC-FIELD-COMMENT: transformPrivateFieldInitializer
        // (_tsc.js:96299-96308) wraps the transformPropertyOrClassStaticBlock
        // statement — which carries the property's comment range, or
        // NoComments for an auto-accessor backing field / a NoComments
        // property (_tsc.js:97444-97466) — in a static block that carries no
        // comment range of its own.
        let statement_has_no_comments = self
            .generated_auto_accessor_backings
            .contains(&operation.original.node())
            || self
                .context
                .arena()
                .metadata(operation.original)
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::NO_COMMENTS));
        if statement_has_no_comments {
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
        }
        let body = self.create_block(vec![statement], true)?;
""")
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""        self.context
            .arena_mut()?
            .set_semantic_original_node(block, operation.original)?;
        let record = self.context.arena().node(operation.original)?;
        let positions = self
            .context
            .arena()
            .source(operation.original.source())?
            .syntax()
            .positions();
        let range = SourceRange::from_raw(record.pos, record.end, positions).map_err(|error| {
            TransformError::InvalidSourceRange {
                node: operation.original,
                error,
            }
        })?;
        self.context
            .arena_mut()?
            .metadata_mut(block)
            .set_comment_range(crate::CommentRange::new(operation.original.source(), range));
        Ok(block)
    }
""",
"""        self.context
            .arena_mut()?
            .set_semantic_original_node(block, operation.original)?;
        self.context
            .arena_mut()?
            .metadata_mut(block)
            .set_comment_range(crate::CommentRange::new(
                self.source,
                SourceRange::Synthesized,
            ));
        Ok(block)
    }
""")

# ---- P9 EF7-SCOPED-NUMBERED-NAMES -----------------------------------------------------
patch("crates/emitter/src/builtins/generated_bindings.rs",
"""        let mut suffix = 1usize;
        loop {
            let candidate = format!("{source_name}_{suffix}");
            if self.reserve_in_source(candidate.clone()) {
                if reserve_in_nested_scopes {
                    self.scopes[0]
                        .names_reserved_in_descendants
                        .push(candidate.clone());
                } else {
                    // makeUniqueName(scoped = false): generatedNames.add.
                    self.generated_names.insert(candidate.clone());
                }
                return candidate;
            }
            suffix += 1;
        }
""",
"""        let mut suffix = 1usize;
        loop {
            let candidate = format!("{source_name}_{suffix}");
            if reserve_in_nested_scopes {
                // EF7-SCOPED-NUMBERED-NAMES: makeUniqueName(scoped = true)
                // reserves the spelling in the CURRENT name-generation scope
                // (`reserveNameInNestedScopes`, released when that scope
                // pops), so sibling function bodies each start again at `_1`
                // (`args_1`, `x_1` of the transformES2017/ES2018 parameter
                // rewrites).
                if self.reserve_in_current(candidate.clone(), true, true) {
                    return candidate;
                }
            } else if self.reserve_in_source(candidate.clone()) {
                // makeUniqueName(scoped = false): generatedNames.add.
                self.generated_names.insert(candidate.clone());
                return candidate;
            }
            suffix += 1;
        }
""")

# ---- P12 EF7-YIELD-PARENS follow-up: `yield yield 0` ----------------------------------
patch("crates/emitter/src/builtins/es2017.rs",
"""            NodeData::PrefixUnaryExpression(_)
            | NodeData::PostfixUnaryExpression(_)
            | NodeData::TypeOfExpression(_)
            | NodeData::VoidExpression(_)
            | NodeData::DeleteExpression(_)
            | NodeData::AwaitExpression(_) => true,
""",
"""            NodeData::PrefixUnaryExpression(_)
            | NodeData::PostfixUnaryExpression(_)
            | NodeData::TypeOfExpression(_)
            | NodeData::VoidExpression(_)
            | NodeData::DeleteExpression(_) => true,
            // createYieldExpression only guards a comma operand
            // (parenthesizeExpressionForDisallowedComma): `yield yield 0`.
            NodeData::AwaitExpression(_) => false,
""")

# ---- P13 EF7-SCOPED-NUMBERED-NAMES: `arguments_N` stays file-wide ---------------------
patch("crates/emitter/src/builtins/es2017.rs",
"""        let binding = self.allocate_numbered_binding("arguments")?;
""",
"""        let binding = self.allocate_file_wide_numbered_binding("arguments")?;
""")
patch("crates/emitter/src/builtins/es2017.rs",
"""    fn allocate_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
""",
"""    /// `factory.createUniqueName("arguments")` (_tsc.js:101326) carries no
    /// ReservedInNestedScopes flag: `makeUniqueName(scoped = false)` numbers
    /// the lexical `arguments` capture file-wide.
    fn allocate_file_wide_numbered_binding(
        &mut self,
        source_name: &str,
    ) -> Result<TargetBinding, TransformError> {
        TargetBinding::allocate_numbered(
            self.context,
            source_name.to_owned(),
            self.generated_bindings.allocate_local_numbered(source_name),
        )
    }

    fn allocate_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
""")

# ---- P16 EF7-DECORATED-STATIC-FIELD-COMMENT (constructor/IIFE statement path) ---------
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, operation.original)?;
        if let Some(source_map_range) = self.property_source_map_range(operation.original)? {
            let leading_synthesized = self.property_name_is_synthesized(operation.original)?;
""",
"""        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, operation.original)?;
        // transformPropertyOrClassStaticBlock (_tsc.js:97444-97466) and
        // generateInitializedPropertyExpressionsOrClassStaticBlock
        // (_tsc.js:97467-97488): `addEmitFlags(…, getEmitFlags(property) &
        // NoComments)`, and an auto-accessor backing field is NoComments.
        if self
            .generated_auto_accessor_backings
            .contains(&operation.original.node())
            || self
                .context
                .arena()
                .metadata(operation.original)
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::NO_COMMENTS))
        {
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
        }
        if let Some(source_map_range) = self.property_source_map_range(operation.original)? {
            let leading_synthesized = self.property_name_is_synthesized(operation.original)?;
""")

# ---- P17 EF7-DECORATED-STATIC-FIELD-COMMENT (< ES2022 accessor expansion) ------------
# tsc's transformESDecorators expands only DESCRIPTOR (private) accessors itself
# (`hasAccessorModifier(node) && descriptorName`, _tsc.js:100101): their backing field is
# NoComments, so the static storage initializer prints no comment. A public decorated
# accessor is expanded later by transformClassFields, whose pending static initializer
# takes the accessor declaration's own comment range (`setCommentRange(expression,
# property)`, _tsc.js:97485) — the case the Rust comment-source carries.
patch("crates/emitter/src/builtins/standard_decorators.rs",
"""        if plan.is_static && self.target < ScriptTarget::ES2022 {
            self.context
                .arena_mut()?
                .metadata_mut(field)
                .class_field_initializer_comment_source = Some(plan.original);
        }
""",
"""        if plan.is_static && self.target < ScriptTarget::ES2022 && plan.descriptor_name.is_none() {
            self.context
                .arena_mut()?
                .metadata_mut(field)
                .class_field_initializer_comment_source = Some(plan.original);
        }
""")

# ---- P18 EF7-EXPORT-EQUALS-EMPTY-ASSIGNED-NAME ----------------------------------------
patch("crates/emitter/src/builtins/standard_decorators.rs",
"""            NodeData::ExportAssignment(data) => {
                let assigned = if data.is_export_equals == Some(true) {
                    ""
                } else {
                    "default"
                };
                self.record_named_evaluation_text(data.expression, assigned)?;
""",
"""            NodeData::ExportAssignment(data) => {
                let assigned = if data.is_export_equals == Some(true) {
                    ""
                } else {
                    "default"
                };
                // `visitExportAssignment`: `transformNamedEvaluation(…,
                // canIgnoreEmptyStringLiteralInAssignedName(node.expression))`
                // — the `""` assigned name of `export =` is dropped for an
                // anonymous class expression without class/constructor-
                // parameter decorators (_tsc.js:100229-100236).
                let ignore_empty = assigned.is_empty()
                    && self.export_assignment_ignores_empty_assigned_name(data.expression)?;
                if !ignore_empty {
                    self.record_named_evaluation_text(data.expression, assigned)?;
                }
""")
patch("crates/emitter/src/builtins/standard_decorators.rs",
"""    fn anonymous_class_needing_assigned_name(
        &self,
        initializer: Option<NodeId>,
    ) -> Result<Option<TransformNode>, TransformError> {
""",
"""    /// `canIgnoreEmptyStringLiteralInAssignedName` (_tsc.js:100229-100236):
    /// an anonymous class expression whose class and constructor parameters
    /// carry no decorators.
    fn export_assignment_ignores_empty_assigned_name(
        &self,
        expression: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let Some(expression) = expression else {
            return Ok(false);
        };
        let inner = self.skip_outer_expressions(self.node(expression))?;
        let NodeData::ClassExpression(data) = &self.context.arena().node(inner)?.data else {
            return Ok(false);
        };
        if data.name.is_some() || !self.decorator_expressions(data.modifiers)?.is_empty() {
            return Ok(false);
        }
        for member in self.array_nodes(data.members)? {
            let NodeData::Constructor(constructor) = &self.context.arena().node(member)?.data
            else {
                continue;
            };
            if constructor.body.is_none() {
                continue;
            }
            for parameter in self.array_nodes(constructor.parameters)? {
                if let NodeData::Parameter(parameter) = &self.context.arena().node(parameter)?.data
                {
                    if !self.decorator_expressions(parameter.modifiers)?.is_empty() {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    fn anonymous_class_needing_assigned_name(
        &self,
        initializer: Option<NodeId>,
    ) -> Result<Option<TransformNode>, TransformError> {
""")

# ---- P11 clippy hygiene (touched crate: tsc-rs-emitter) -------------------------------
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""pub(super) fn transform_source(
    context: &mut TransformationContext,
""",
"""#[allow(clippy::too_many_arguments)]
pub(super) fn transform_source(
    context: &mut TransformationContext,
""")
# `StringLiteral`/`NoSubstitutionTemplateLiteral` `text` is already a JsString
# (Identifier/PrivateIdentifier/NumericLiteral `text` is a String and keeps `.into()`).
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""            NodeData::StringLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone().into()))""",
"""            NodeData::StringLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone()))""", count=3)
patch("crates/emitter/src/builtins/class_fields/downlevel.rs",
"""            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone().into()))""",
"""            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone()))""", count=2)
patch("crates/emitter/src/builtins/class_fields.rs",
"""(node.flags & NodeFlags::SYNTHESIZED.bits() as i32) == 0""",
"""(node.flags & NodeFlags::SYNTHESIZED.bits()) == 0""")
patch("crates/emitter/src/builtins/class_fields.rs",
"""(record.flags & NodeFlags::SYNTHESIZED.bits() as i32) != 0""",
"""(record.flags & NodeFlags::SYNTHESIZED.bits()) != 0""")
patch("crates/emitter/src/builtins/class_fields.rs",
"""    fn visit_function_parts(
        &mut self,
""",
"""    #[allow(clippy::type_complexity)]
    fn visit_function_parts(
        &mut self,
""")
patch("crates/emitter/src/builtins/es2015.rs",
"""        let created = factory.create_node(
            node.source(),
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.clone(),
                text,
            }),
            TransformFlags::NONE,
        )?;
        created
    };
""",
"""        factory.create_node(
            node.source(),
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.clone(),
                text,
            }),
            TransformFlags::NONE,
        )?
    };
""")
patch("crates/emitter/src/builtins/system.rs",
"""                )?
                .map(JsString::from)
                // tryRenameExternalModule""",
"""                )?
                // tryRenameExternalModule""")
patch("crates/emitter/src/builtins.rs",
"""            let path = JsString::from(dependency.path);""",
"""            let path = dependency.path;""")
patch("crates/emitter/src/declaration_map.rs",
"""        generator.raw_sources().iter().cloned().collect(),""",
"""        generator.raw_sources().to_vec(),""")
patch("crates/emitter/src/execute.rs",
"""                        generator.raw_sources().iter().cloned().collect(),""",
"""                        generator.raw_sources().to_vec(),""")
patch("crates/emitter/src/execute.rs",
"""    if options
        .out_file
        .as_ref()
        .is_some_and(|path| !path.is_empty())
    {
        if options.import_helpers == Some(true) {
            return unsupported("importHelpers");
        }
    }
""",
"""    if options
        .out_file
        .as_ref()
        .is_some_and(|path| !path.is_empty())
        && options.import_helpers == Some(true)
    {
        return unsupported("importHelpers");
    }
""")
patch("crates/emitter/src/execute.rs",
"""        source_root: source_root_field(options).into(),
        sources_directory_path: source_map_directory_for_output(
            lane,
            options,
            javascript_path,
            source_path,
        )
        .into(),
        current_directory: lane.current_directory.clone().into(),
""",
"""        source_root: source_root_field(options),
        sources_directory_path: source_map_directory_for_output(
            lane,
            options,
            javascript_path,
            source_path,
        ),
        current_directory: lane.current_directory.clone(),
""")
print("r11 part 1 applied")
