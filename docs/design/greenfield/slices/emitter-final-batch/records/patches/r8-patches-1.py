#!/usr/bin/env python3
"""r8 part 1 (EF2-AWAIT-USING-MISSING-NAME): an empty `using` declaration list in a for-of head
gets tsc's synthesized declaration `createVariableDeclaration(createTempVariable(undefined))` (temp
family, `_e`), and the loop binding is `getGeneratedNameForNode(temp)` (`_e_1`), not a `value`
numbered name."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/builtins/es_next.rs", [
("""        let base = binding_name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned))
            .unwrap_or_else(|| "value".to_owned());
        let temp_binding = self.allocate_generated_binding(&base)?;
        let temp = self.create_generated_identifier(&temp_binding)?;
""", """        // `firstOrUndefined(declarations) || createVariableDeclaration(
        // createTempVariable(undefined))` (_tsc.js:103430-103433): a
        // `for (await using of x)` head parses as an EMPTY declaration
        // list, so the using variable is a synthesized temp (`_e`) and the
        // loop binding `getGeneratedNameForNode(temp)` derives from it
        // (`_e_1`).
        let synthesized_declaration_name = match binding_name {
            Some(_) => None,
            None => {
                let provisional = self.allocate_generated_name("_tmp");
                let temp_binding = TargetBinding::allocate(self.context, provisional)?;
                let name = self.create_generated_identifier(&temp_binding)?;
                Some((temp_binding, name))
            }
        };
        let temp_binding = match (&binding_name, &synthesized_declaration_name) {
            (Some(name), _) => {
                let base = self
                    .identifier_text(self.node(*name))
                    .map(str::to_owned)
                    .unwrap_or_else(|| "value".to_owned());
                self.allocate_generated_binding(&base)?
            }
            (None, Some((temp_binding, _))) => {
                let provisional = self.allocate_generated_name(temp_binding.provisional_name());
                TargetBinding::allocate_numbered_derived(self.context, temp_binding, provisional)?
            }
            (None, None) => unreachable!("an empty using list synthesizes its declaration"),
        };
        let temp = self.create_generated_identifier(&temp_binding)?;
"""),
("""        let using_name = binding_name.unwrap_or(temp.node());
""", """        let using_name = match (binding_name, &synthesized_declaration_name) {
            (Some(name), _) => name,
            (None, Some((_, name))) => name.node(),
            (None, None) => unreachable!("an empty using list synthesizes its declaration"),
        };
"""),
])
print("r8 part 1 applied")
