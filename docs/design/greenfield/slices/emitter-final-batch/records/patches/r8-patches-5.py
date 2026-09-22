#!/usr/bin/env python3
"""r8 part 5 (EF2-ALIAS-NUMBERING, references): a print-time substitution identifier takes the
binding's finalized spelling recorded by the finalize walk, not its provisional name (the block
class reference `Foo_2.func()` after the walk renumbered the declaration)."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:90])
        s = s.replace(old, new)
    open(path, "w").write(s)
W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/builtins/es2015.rs", [
("""    let clone = {
        let mut factory = context.substitution_factory()?;
        let created = factory.create_node(
            node.source(),
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: binding.provisional_name().to_owned(),
                text: binding.provisional_name().to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        created
    };
""", """    // The finalize walk already spelled this binding (its declaration is
    // in the tree); a substitution clone created during the print reuses
    // that spelling, falling back to the provisional name only for a
    // binding the walk never met.
    let text = context
        .generated_binding_name(binding.id())
        .map(str::to_owned)
        .unwrap_or_else(|| binding.provisional_name().to_owned());
    let clone = {
        let mut factory = context.substitution_factory()?;
        let created = factory.create_node(
            node.source(),
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.clone(),
                text,
            }),
            TransformFlags::NONE,
        )?;
        created
    };
"""),
])
print("r8 part 5 applied")
