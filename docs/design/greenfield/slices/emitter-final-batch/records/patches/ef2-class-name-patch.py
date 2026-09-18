#!/usr/bin/env python3
"""EF2-CLASS-NAME: ES2015 getGeneratedNameForNode resolves the base through the original chain (getNodeForGeneratedName)."""
path = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2015.rs"
src = open(path).read()
old = """    fn get_generated_name_for_node(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let key = (node.source(), node.node());
        if let Some(binding) = self
            .print_state
            .generated_names_for_nodes
            .get(&key)
            .cloned()
        {
            return self.create_generated_identifier(&binding);
        }
        // generateNameForNode arms (`_tsc.js:120876-120933`): identifier →
        // text-numbered; class expression → the "class" family; named
        // class/function declarations recurse on the name, unnamed →
        // "default"; everything else (pattern parameters, computed names)
        // → the temp family.
        let name_text = match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        };
        let kind_base = match &self.context.arena().node(node)?.data {
"""
new = """    fn get_generated_name_for_node(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // The printer resolves a `GeneratedIdentifierFlags.Node` name through
        // `getNodeForGeneratedName` (`_tsc.js:28084-28102`): it follows the
        // `original` chain from the requesting node to its root before
        // `generateNameForNode` classifies that node, and `generateNameCached`
        // keys the spelling on the resolved node. A decorated class lowered
        // by the ES-decorators pass is an unnamed class expression whose
        // original is the named declaration, so its ES5 constructor function
        // is `D_1`, never a member of the `class_1` family.
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
        // generateNameForNode arms (`_tsc.js:120876-120933`): identifier →
        // text-numbered; class expression → the "class" family; named
        // class/function declarations recurse on the name, unnamed →
        // "default"; everything else (pattern parameters, computed names)
        // → the temp family.
        let name_text = match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        };
        let kind_base = match &self.context.arena().node(node)?.data {
"""
assert src.count(old) == 1
src = src.replace(old, new)
open(path, "w").write(src)
print("EF2-CLASS-NAME patch applied")
