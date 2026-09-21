#!/usr/bin/env python3
"""r7 part 1: EF2-ASYNC-GEN-BODY-FLAG (createAsyncGeneratorHelper emit flags);
EF2-TOP-LEVEL-FOR-AWAIT (visitForOfStatement lowers every awaitModifier, no async-function gate)."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

P = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2018.rs"
patch(P, [
("""            inner_flags,
        )?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncGenerator)?;
""", """            inner_flags,
        )?;
        // `createAsyncGeneratorHelper` marks the generator function
        // `AsyncFunctionBody | ReuseTempVariableScope`: the ES2015 pass then
        // enters it through `AsyncFunctionBodyExcludes`, keeping the
        // enclosing method's `NonStaticClassElement` fact for `super`
        // property lowering (`_super.prototype.x`).
        self.context
            .arena_mut()?
            .metadata_mut(inner)
            .add_flags(EmitFlags::ASYNC_FUNCTION_BODY | EmitFlags::REUSE_TEMP_VARIABLE_SCOPE);
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncGenerator)?;
"""),
("""            NodeData::ForOfStatement(data)
                if data.await_modifier.is_some() && self.current_function_is_async() =>
            {
""", """            // `visitForOfStatement` (_tsc.js: transformES2018) lowers every
            // `for await` it reaches, including a module-level statement
            // outside any async function (the `await` stays as written).
            NodeData::ForOfStatement(data) if data.await_modifier.is_some() => {
"""),
("""    ) -> Result<bool, TransformError> {
        if !self.current_function_is_async() {
            return Ok(false);
        }
        let Some(mut statement) = data.statement.map(|statement| self.node(statement)) else {
""", """    ) -> Result<bool, TransformError> {
        let Some(mut statement) = data.statement.map(|statement| self.node(statement)) else {
"""),
])
print("r7 part 1 applied")
