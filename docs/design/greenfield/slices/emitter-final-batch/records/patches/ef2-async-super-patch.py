#!/usr/bin/env python3
"""EF2-ASYNC-SUPER: the ES2017 `_super` access object exists only at >= ES2015 and never for async generators."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2017.rs"
s = open(p).read()
old = """        let resolver_node = self.resolver_node(function)?;
        let has_assignment = self.resolver.has_node_check_flag(
            resolver_node,
            NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ASSIGNMENT_IN_ASYNC.bits() as u32,
        )?;
"""
new = """        // `emitSuperHelpers` (_tsc.js:101243, 101378): below ES2015 the
        // ES2015 pass lowers `super` itself, and an async generator method's
        // helpers belong to the ES2018 pass (which applies the same floor).
        if self.target < ScriptTarget::ES2015 || self.is_async_generator(function)? {
            return Ok(AsyncSuperCapture::default());
        }
        let resolver_node = self.resolver_node(function)?;
        let has_assignment = self.resolver.has_node_check_flag(
            resolver_node,
            NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ASSIGNMENT_IN_ASYNC.bits() as u32,
        )?;
"""
assert s.count(old) == 1
s = s.replace(old, new)
old2 = """    fn plan_async_super_capture(
        &mut self,
"""
new2 = """    fn is_async_generator(&self, function: TransformNode) -> Result<bool, TransformError> {
        let original = self.context.arena().get_original_node(function);
        Ok(match &self.context.arena().node(original)?.data {
            NodeData::MethodDeclaration(data) => data.asterisk_token.is_some(),
            NodeData::FunctionDeclaration(data) => data.asterisk_token.is_some(),
            NodeData::FunctionExpression(data) => data.asterisk_token.is_some(),
            _ => false,
        })
    }

    fn plan_async_super_capture(
        &mut self,
"""
assert s.count(old2) == 1
s = s.replace(old2, new2)
open(p, "w").write(s)
print("EF2-ASYNC-SUPER patch applied")
