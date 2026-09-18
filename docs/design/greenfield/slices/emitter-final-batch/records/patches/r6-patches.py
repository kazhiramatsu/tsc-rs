#!/usr/bin/env python3
"""r6: ES2018 super gate restructure; EF4-NESTED-THIS; EF4-ARROW-CRASH; EF4-DEFAULT-NAME v2."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

# A. ES2018: keep the capture structure; gate only the helper emission / super rewriting.
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2018.rs", [
("""        // `emitSuperHelpers` (_tsc.js:102657): below ES2015 the ES2015 pass
        // lowers `super` itself, so the async generator keeps its direct uses.
        let owns_super_capture = mode.is_async_generator() && self.target >= ScriptTarget::ES2015;
        let introduces_super_boundary = owns_super_capture || kind != SyntaxKind::ArrowFunction;
""", """        let owns_super_capture = mode.is_async_generator();
        let introduces_super_boundary = owns_super_capture || kind != SyntaxKind::ArrowFunction;
"""),
("""            let capture = if owns_super_capture {
                let facts = self.collect_async_generator_super_facts(body)?;
                let binding = facts
                    .owns_access
""", """            let capture = if owns_super_capture {
                let mut facts = self.collect_async_generator_super_facts(body)?;
                // `emitSuperHelpers` (_tsc.js:102657): below ES2015 the ES2015
                // pass lowers `super` itself, so the async generator keeps its
                // direct uses and emits no `_super` access object.
                if self.target < ScriptTarget::ES2015 {
                    facts.owns_access = false;
                    facts.has_element_access = false;
                    facts.has_assignment = false;
                    facts.captured_properties.clear();
                }
                let binding = facts
                    .owns_access
"""),
("""    fn super_capture_is_active(&self) -> bool {
        self.async_generator_super_captures
            .last()
            .is_some_and(Option::is_some)
    }
""", """    fn super_capture_is_active(&self) -> bool {
        self.async_generator_super_captures
            .last()
            .and_then(Option::as_ref)
            .is_some_and(|capture| capture.owns_access)
    }
"""),
])

# B. EF4-NESTED-THIS: transform-time `this` in a class element name follows the current
# class lexical environment (a nested class resets it: visitInNewClassLexicalEnvironment).
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/class_fields/downlevel.rs", [
("""    fn visit_computed_property_expression(
        &mut self,
        expression: Option<NodeId>,
        crosses_class_boundary: bool,
    ) -> Result<TransformNode, TransformError> {
        let enclosing = crosses_class_boundary
            .then(|| self.static_binding_frames.enclosing_class_evaluation())
            .flatten();
        let _enclosing_scope = enclosing.map(|bindings| {
            self.static_binding_frames
                .enter(StaticBindingFrame::StaticEvaluation(Some(bindings)))
        });
        self.visit_required(expression, SyntaxKind::ComputedPropertyName, "expression")
    }
""", """    fn visit_computed_property_expression(
        &mut self,
        expression: Option<NodeId>,
        _crosses_class_boundary: bool,
    ) -> Result<TransformNode, TransformError> {
        // visitThisExpression (_tsc.js:97136-97150) substitutes with the
        // CURRENT class lexical environment, which
        // visitInNewClassLexicalEnvironment resets when a nested class is
        // entered: a `this` inside a nested class's element name is left as
        // written even though the name is evaluated in the enclosing scope
        // (`class_2.prototype[this.key]`, TS2465 recovery). The print-time
        // `lexicalEnvironment.previous` switch stays in `before_emit_node`.
        self.visit_required(expression, SyntaxKind::ComputedPropertyName, "expression")
    }
"""),
])

# C. EF4-ARROW-CRASH: mergeEmitNode never copies autoGenerate; a non-identifier that takes an
# identifier as its original (a concise arrow body turned into a block) must not inherit it.
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/metadata.rs", [
("""    pub(crate) fn merge_from(&mut self, source: &Self) {
""", """    /// tsc's `mergeEmitNode` copies no `autoGenerate` information; a node that
    /// takes an identifier as its original (a concise arrow body converted to
    /// a block) keeps no generated-binding identity.
    pub(crate) fn clear_generated_binding(&mut self) {
        self.generated_binding_id = None;
        self.generated_binding_temp_ordinal = None;
        self.generated_binding_base = None;
        self.generated_binding_preferred_base = None;
        self.generated_binding_role_suffix = None;
        self.generated_binding_file_level_optimistic = false;
        self.generated_binding_planned_name_authoritative = false;
        self.generated_binding_reserved_in_nested_scopes = false;
        self.generated_binding_loop_variable = false;
        self.generated_binding_private_temp = false;
        self.generated_binding_derived_from = None;
    }

    pub(crate) fn merge_from(&mut self, source: &Self) {
"""),
])
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/factory.rs", [
("""        let source_metadata = original.and_then(|original| self.metadata.get(&original).cloned());
        let metadata = self.metadata.entry(node).or_default();
        metadata.original = original;
        metadata.original_is_semantic = false;
        if let Some(source_metadata) = source_metadata {
            metadata.merge_from(&source_metadata);
            metadata.original = original;
        }
        Ok(())
    }
""", """        let source_metadata = original.and_then(|original| self.metadata.get(&original).cloned());
        let node_is_member_name = matches!(
            self.node(node)?.data,
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_)
        );
        let metadata = self.metadata.entry(node).or_default();
        metadata.original = original;
        metadata.original_is_semantic = false;
        if let Some(source_metadata) = source_metadata {
            let generated_binding_before = metadata.generated_binding_id;
            metadata.merge_from(&source_metadata);
            metadata.original = original;
            if !node_is_member_name && generated_binding_before.is_none() {
                metadata.clear_generated_binding();
            }
        }
        Ok(())
    }
"""),
])

# D. EF4-DEFAULT-NAME v2: the generated name of a declaration is one binding across passes;
# walk the original chain for the declaration name an earlier pass generated.
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2015.rs", [
("""        let node = self.context.arena().get_original_node(node);
        let key = (node.source(), node.node());
        if let Some(binding) = self
            .print_state
            .generated_names_for_nodes
            .get(&key)
            .cloned()
        {
            return self.create_generated_identifier(&binding);
        }
""", """        let requested = node;
        let node = self.context.arena().get_original_node(node);
        let key = (node.source(), node.node());
        if let Some(binding) = self
            .print_state
            .generated_names_for_nodes
            .get(&key)
            .cloned()
        {
            return self.create_generated_identifier(&binding);
        }
        // `generateNameCached(node)` is one slot per declaration for every
        // pass: transformTypeScript already named an anonymous default
        // declaration (`default_1`) and left that generated identifier as the
        // declaration's name on the intermediate node of the original chain.
        if let Some(binding) = self.generated_declaration_name_binding_on_chain(requested) {
            self.print_state
                .generated_names_for_nodes
                .insert(key, binding.clone());
            return self.create_generated_identifier(&binding);
        }
"""),
("""    /// The generated binding an identifier already carries (written by an
    /// earlier transform's `write_generated_metadata`).
    fn generated_binding_of_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
""", """    /// Walk the original chain from `node`; the first class or function
    /// declaration whose current name is a generated identifier donates that
    /// identifier's binding.
    fn generated_declaration_name_binding_on_chain(
        &self,
        node: TransformNode,
    ) -> Option<TargetBinding> {
        let arena = self.context.arena();
        let mut current = node;
        let mut remaining = 64usize;
        loop {
            let name = match arena.node(current).ok()?.data {
                NodeData::ClassDeclaration(ref data) => data.name,
                NodeData::FunctionDeclaration(ref data) => data.name,
                NodeData::ClassExpression(ref data) => data.name,
                NodeData::FunctionExpression(ref data) => data.name,
                _ => None,
            };
            if let Some(binding) = name
                .map(|id| TransformNode::new(current.source(), id))
                .and_then(|name| self.generated_binding_of_identifier(name))
            {
                return Some(binding);
            }
            let original = arena.metadata(current).and_then(|metadata| metadata.original())?;
            if original == current || remaining == 0 {
                return None;
            }
            remaining -= 1;
            current = original;
        }
    }

    /// The generated binding an identifier already carries (written by an
    /// earlier transform's `write_generated_metadata`).
    fn generated_binding_of_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
"""),
])
print("r6 patches applied")
