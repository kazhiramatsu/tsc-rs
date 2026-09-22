#!/usr/bin/env python3
"""EF4-DEFAULT-NAME: ES2015 getName reuses the generated binding an assigned name already carries
(tsc: a generated assigned identifier resolves through getNodeForGeneratedName to the same cached name)."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2015.rs"
s = open(p).read()
old = """        if name_is_plain_identifier {
            let name = name.expect("plain identifier name");
            let clone = self.clone_node(name)?;
"""
new = """        // `getName` (_tsc.js:24788-24799) takes the generated-name path for a
        // generated assigned name, and `getNodeForGeneratedName`
        // (_tsc.js:28084-28102) resolves that identifier through its own
        // original chain to the declaration it was generated for — the same
        // cache slot `generateNameCached` uses for this node. An anonymous
        // default class named `default_1` by transformTypeScript therefore
        // keeps `default_1` as its ES5 constructor name instead of a second
        // numbered allocation.
        if let Some(binding) = name
            .filter(|_| !name_is_plain_identifier)
            .and_then(|name| self.generated_binding_of_identifier(name))
        {
            let origin = self.context.arena().get_original_node(node);
            self.print_state
                .generated_names_for_nodes
                .entry((origin.source(), origin.node()))
                .or_insert_with(|| binding.clone());
            return self.create_generated_identifier(&binding);
        }
        if name_is_plain_identifier {
            let name = name.expect("plain identifier name");
            let clone = self.clone_node(name)?;
"""
assert s.count(old) == 1
s = s.replace(old, new)
# helper next to get_generated_name_for_node
old2 = """    /// `getGeneratedNameForNode(node)` — `generateNameCached`: ONE binding
    /// per parse-tree node, cached in the print state (print-time
"""
new2 = """    /// The generated binding an identifier already carries (written by an
    /// earlier transform's `write_generated_metadata`).
    fn generated_binding_of_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
        let metadata = self.context.arena().metadata(name)?;
        let id = metadata.generated_binding_id()?;
        let NodeData::Identifier(identifier) = &self.context.arena().node(name).ok()?.data else {
            return None;
        };
        Some(TargetBinding::from_existing(
            id,
            identifier.text.clone(),
            metadata.generated_binding_base().map(str::to_owned),
            metadata
                .generated_binding_preferred_base()
                .map(str::to_owned),
            metadata.generated_binding_role_suffix().map(str::to_owned),
            metadata.generated_binding_is_file_level_optimistic(),
            metadata.generated_binding_planned_name_is_authoritative(),
            metadata.generated_binding_reserved_in_nested_scopes(),
            metadata.generated_binding_is_private_temp(),
        ))
    }

    /// `getGeneratedNameForNode(node)` — `generateNameCached`: ONE binding
    /// per parse-tree node, cached in the print state (print-time
"""
assert s.count(old2) == 1
s = s.replace(old2, new2)
open(p, "w").write(s)
print("EF4-DEFAULT-NAME patch applied")
