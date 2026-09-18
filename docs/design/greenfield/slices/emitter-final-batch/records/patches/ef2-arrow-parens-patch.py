#!/usr/bin/env python3
"""EF2-ARROW-PARENS: structural arrow-function updates apply parenthesizeConciseBodyOfArrowFunction (updateArrowFunction)."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/factory.rs"
s = open(p).read()
old = """        self.parenthesize_computed_property_name_expression(source, data)?;
        self.parenthesize_export_assignment_expression(source, data)
    }
"""
new = """        self.parenthesize_computed_property_name_expression(source, data)?;
        self.parenthesize_export_assignment_expression(source, data)?;
        self.parenthesize_updated_arrow_concise_body(source, data)
    }

    /// tsc-port: updateArrowFunction @6.0.3 (through createArrowFunction's
    /// `parenthesizeConciseBodyOfArrowFunction`)
    /// tsc-span: _tsc.js:22701-22735
    ///
    /// A transform that replaces an arrow's concise body (the TypeScript
    /// pass erasing `({ … } as T)[x]` into partially emitted expressions)
    /// reaches this rule instead of the constructor; the leftmost object
    /// literal or comma sequence still needs the virtual parentheses.
    fn parenthesize_updated_arrow_concise_body(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let NodeData::ArrowFunction(arrow) = data else {
            return Ok(());
        };
        let Some(body) = arrow.body.and_then(|body| self.arena.node_ref(source, body)) else {
            return Ok(());
        };
        let parenthesized = self.parenthesize_concise_body(body)?;
        arrow.body = Some(parenthesized.node);
        Ok(())
    }
"""
assert s.count(old) == 1
s = s.replace(old, new)
open(p, "w").write(s)
print("EF2-ARROW-PARENS patch applied")
