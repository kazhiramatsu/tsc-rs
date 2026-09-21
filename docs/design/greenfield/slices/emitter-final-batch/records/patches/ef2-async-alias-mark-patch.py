#!/usr/bin/env python3
"""EF2-ASYNC-ALIAS-MARK: checkAsyncFunctionReturnType (_tsc.js:82513) marks the ES5 async
return-type alias referenced during checking; the Rust port only reached the marker through
the unchecked-file emit walk, so a checked file elided the import of the promise constructor."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/checker/src/functions.rs", [
("""    /// `return_type_error_location` differs for a JSDoc `@type`
    /// reference whose resolved call signature supplies the actual
    /// return annotation. `markLinkedReferences` is emit-only.
""", """    /// `return_type_error_location` differs for a JSDoc `@type`
    /// reference whose resolved call signature supplies the actual
    /// return annotation. The ES5 branch marks the promise-constructor
    /// alias referenced here (`markLinkedReferences(node, AsyncFunction)`,
    /// _tsc.js:82513) so import elision keeps the constructor's import; the
    /// emit-time walk only covers unchecked files.
"""),
("""        } else {
            if self.tables.is_error_type(return_type) {
                return Ok(());
            }
            let promise_constructor_name = self.get_entity_name_from_type_node(return_type_node);
""", """        } else {
            self.mark_linked_references_async_function(node)?;
            if self.tables.is_error_type(return_type) {
                return Ok(());
            }
            let promise_constructor_name = self.get_entity_name_from_type_node(return_type_node);
"""),
])

patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/checker/src/modules.rs", [
("""    /// tsc-port: markAsyncFunctionAliasReferenced @6.0.3
    /// tsc-hash: a9ab88c572baf7efb5904eb94595de4c3316c636d7b18b0c3218c54e6c38d08e
    /// tsc-span: _tsc.js:71828-71835
    fn mark_async_function_alias_referenced(&mut self, location: NodeId) -> CheckResult<()> {
""", """    /// `markLinkedReferences(location, ReferenceHint.AsyncFunction)`
    /// (_tsc.js:71662-71679): the front-door guards (verbatimModuleSyntax,
    /// ambient locations) followed by the async-function marker.
    pub(crate) fn mark_linked_references_async_function(
        &mut self,
        location: NodeId,
    ) -> CheckResult<()> {
        if self.options.verbatim_module_syntax == Some(true) {
            return Ok(());
        }
        if self
            .binder
            .flags_of(location)
            .intersects(tsc_types::NodeFlags::AMBIENT)
            && !matches!(
                self.kind_of(location),
                SyntaxKind::PropertySignature | SyntaxKind::PropertyDeclaration
            )
        {
            return Ok(());
        }
        self.mark_async_function_alias_referenced(location)
    }

    /// tsc-port: markAsyncFunctionAliasReferenced @6.0.3
    /// tsc-hash: a9ab88c572baf7efb5904eb94595de4c3316c636d7b18b0c3218c54e6c38d08e
    /// tsc-span: _tsc.js:71828-71835
    fn mark_async_function_alias_referenced(&mut self, location: NodeId) -> CheckResult<()> {
"""),
])
print("EF2-ASYNC-ALIAS-MARK patch applied")
