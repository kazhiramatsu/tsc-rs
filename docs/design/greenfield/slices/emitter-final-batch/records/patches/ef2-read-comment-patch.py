#!/usr/bin/env python3
"""EF2-READ-COMMENT: a synthesized argument list emits an element's end-position comments only when the
element does not end where the call itself ends (emitNodeListItems: previousSibling.end !== parentNode.end)."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/printer.rs"
s = open(p).read()
old = """            if synthesized_array {
                // A transformed call owns a fresh argument-list container.
                // Retained source expressions therefore keep the comments at
                // their own end boundary (notably classic JSX children such
                // as `{value/* comment */}`). Parsed argument arrays continue
                // to use their comma/close-delimiter ownership below.
                self.emit_list_element_end_comments_in_container(
                    transformation,
                    node,
                    expression_context.comments(),
                    writer,
                )?;
            }
"""
new = """            if synthesized_array {
                // A transformed call owns a fresh argument-list container.
                // Retained source expressions therefore keep the comments at
                // their own end boundary (notably classic JSX children such
                // as `{value/* comment */}`). Parsed argument arrays continue
                // to use their comma/close-delimiter ownership below.
                // emitNodeListItems (_tsc.js:120088-120094, 120140-120142)
                // skips that boundary when the element ends exactly where
                // the (ranged) parent ends: the flattener's `__read(value, n)`
                // carries the declaration's range, so the source comment
                // after `value` belongs to the following statement.
                let element_end = transformation.arena().node(node)?.end;
                let parent_end = transformation.arena().node(parent)?.end;
                if element_end == u32::MAX || element_end != parent_end {
                    self.emit_list_element_end_comments_in_container(
                        transformation,
                        node,
                        expression_context.comments(),
                        writer,
                    )?;
                }
            }
"""
assert s.count(old) == 1
open(p, "w").write(s.replace(old, new))
print("EF2-READ-COMMENT patch applied")
