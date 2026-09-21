#!/usr/bin/env python3
"""r8 part 8 (EF2-ALIAS-NUMBERING follow-up): the visit-time colliding-declaration-name substitute
queried `isDeclarationWithCollidingName` for every declaration name; tsc asks only once block-scoped
substitutions are enabled (`enabledSubstitutions & BlockScopedBindings`, set by a block-scoped
declaration list, a block-scoped `for` initializer or a named class expression before their names
are visited). Emitter contract tests with a stub resolver (plain `var` sources) hit
`Resolver(Unavailable { IsDeclarationWithCollidingName })` in the transform."""
W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:90])
        s = s.replace(old, new)
    open(path, "w").write(s)
patch(W + "crates/emitter/src/builtins/es2015.rs", [
("""    fn colliding_declaration_name_substitute(
        &mut self,
        name: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if !matches!(
            self.context.arena().node(name)?.data,
            NodeData::Identifier(_)
        ) || is_internal_name(self.context, name)
        {
            return Ok(None);
        }
""", """    fn colliding_declaration_name_substitute(
        &mut self,
        name: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        // substituteIdentifier runs only once block-scoped substitutions are
        // enabled (_tsc.js:108011-108019); the enabling sites (a block-scoped
        // declaration list or `for` initializer, a named class expression)
        // all precede the visit of the names they cover, so the visit-time
        // gate sees the same state the print-time one would.
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS)
            || !matches!(
                self.context.arena().node(name)?.data,
                NodeData::Identifier(_)
            )
            || is_internal_name(self.context, name)
        {
            return Ok(None);
        }
"""),
])
print("r8 part 8 applied")
