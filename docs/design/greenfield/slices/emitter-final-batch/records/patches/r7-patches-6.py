#!/usr/bin/env python3
"""r7 part 6 (EF5-BARE-CLONE-SPELLING, corrected): keep the metadata inheritance of
`cloned_identifier_spelling` (getName-derived clone chains print the parsed spelling, as at r6) and
make the one upstream bare-clone site explicit — `createExportExpression`'s `cloneNode(name)` prints
`idText`."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/metadata.rs", [
("""        // `cloned_identifier_spelling` stands in for the clone's own
        // pos/end/parent (`setTextRange` + `setParent` upstream), which
        // `mergeEmitNode` never copies: a bare `cloneNode` of a getName clone
        // prints `idText` again.
""", """        // A getName-derived clone chain keeps the parsed spelling: this port's
        // transforms hand later passes their own (position-synthetic) clones
        // where upstream still holds the parsed name, so the mark travels with
        // the chain. The upstream bare-clone sites that print `idText`
        // (`createExportExpression`) clear it explicitly.
        self.cloned_identifier_spelling |= source.cloned_identifier_spelling;
"""),
])
patch(W + "crates/emitter/src/builtins.rs", [
("""        let exports = self.create_identifier("exports")?;
        let property = match name.syntax {
            ModuleExportNameSyntax::ExistingNode(node) => {
                self.context.factory()?.clone_node(node)?
            }
""", """        let exports = self.create_identifier("exports")?;
        let property = match name.syntax {
            ModuleExportNameSyntax::ExistingNode(node) => {
                // `factory.cloneNode(name)` alone (_tsc.js:111841-111846): the
                // clone is synthesized with no parent, so `getTextOfNode`
                // prints `idText` (`exports.Foo`), never the parsed spelling.
                let clone = self.context.factory()?.clone_node(node)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(clone)
                    .cloned_identifier_spelling = false;
                clone
            }
"""),
])
print("r7 part 6 applied")
