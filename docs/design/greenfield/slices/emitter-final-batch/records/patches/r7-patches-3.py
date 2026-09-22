#!/usr/bin/env python3
"""r7 part 3: EF2-BLOCK-SCOPED-DECORATED — the decorated class head's `declName`
(getInternalName below ES2015) and the class wrapper's `getLocalName` variable are getName-style
clones of the parsed class name, so the ES2015 colliding-name substitution renames exactly the
local (outer) name and leaves the internal one alone."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/builtins/legacy_decorators.rs", [
("""        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(declaration, Some(original))?;
        let statement = self.create_variable_statement(vec![declaration], NodeFlags::LET)?;
""", """        let declaration_name = self.create_declaration_head_name(name)?;
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(declaration_name),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(declaration, Some(original))?;
        let statement = self.create_variable_statement(vec![declaration], NodeFlags::LET)?;
"""),
("""    const fn node(&self, id: NodeId) -> TransformNode {
""", """    /// `declName` (_tsc.js:98576-98584): `getInternalName(node, false, true)`
    /// below ES2015, `getLocalName(node, false, true)` otherwise — a clone of
    /// the parsed class name carrying `LocalName` (plus `InternalName` for
    /// ES5) and `NoComments`, so the ES2015 block-scoped-binding substitution
    /// renames the wrapper's local name but leaves this internal one alone.
    /// A generated name (`default_1`) is the generated identifier itself.
    fn create_declaration_head_name(&mut self, name: NodeId) -> Result<NodeId, TransformError> {
        let name_node = self.node(name);
        let is_plain_identifier = matches!(
            self.context.arena().node(name_node)?.data,
            NodeData::Identifier(_)
        ) && self
            .context
            .arena()
            .metadata(name_node)
            .and_then(|metadata| metadata.generated_binding_id())
            .is_none();
        if !is_plain_identifier {
            return Ok(name);
        }
        let clone = self.context.factory()?.clone_node(name_node)?;
        self.context.factory()?.set_text_range(clone, name_node)?;
        let mut flags = EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS;
        if self.target < ScriptTarget::ES2015 {
            flags |= EmitFlags::INTERNAL_NAME;
        }
        self.context.arena_mut()?.metadata_mut(clone).add_flags(flags);
        Ok(clone.node())
    }

    const fn node(&self, id: NodeId) -> TransformNode {
"""),
])
patch(W + "crates/emitter/src/builtins.rs", [
("""        let variable_declaration = {
            let name = self.create_identifier(&name_text)?;
            self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: Some(iife.node()),
                }),
                TransformFlags::NONE,
            )?
        };
""", """        let variable_declaration = {
            // `getLocalName(node, false, false)` (_tsc.js:94487-94494): a clone
            // of the parsed class name (`LocalName | NoComments | NoSourceMap`)
            // whose original reaches the parse tree, so the ES2015
            // block-scoped-binding substitution renames a colliding
            // block-level class (`var Foo_1 = (function () { … }())`).
            let name = self.create_wrapper_local_name(updated_class, &name_text)?;
            self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: Some(iife.node()),
                }),
                TransformFlags::NONE,
            )?
        };
"""),
("""    fn identifier_text(&self, id: NodeId) -> Result<&str, TransformError> {
""", """    /// The class wrapper variable's `getLocalName` clone (see the call site);
    /// a generated class name keeps the synthesized identifier.
    fn create_wrapper_local_name(
        &mut self,
        updated_class: TransformNode,
        name_text: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(updated_class)?.data {
            NodeData::ClassDeclaration(data) => data.name,
            _ => None,
        }
        .map(|name| TransformNode::new(self.source, name));
        let plain_name = match name {
            Some(name)
                if matches!(
                    self.context.arena().node(name)?.data,
                    NodeData::Identifier(_)
                ) && self
                    .context
                    .arena()
                    .metadata(name)
                    .and_then(|metadata| metadata.generated_binding_id())
                    .is_none() =>
            {
                name
            }
            _ => return self.create_identifier(name_text),
        };
        let clone = self.context.factory()?.clone_node(plain_name)?;
        self.context.factory()?.set_text_range(clone, plain_name)?;
        self.context
            .arena_mut()?
            .metadata_mut(clone)
            .add_flags(EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
        Ok(clone)
    }

    fn identifier_text(&self, id: NodeId) -> Result<&str, TransformError> {
"""),
])
print("r7 part 3 applied")
