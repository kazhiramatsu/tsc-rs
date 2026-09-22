#!/usr/bin/env python3
"""r7 part 4: EF4-FILE-THIS-CAPTURE — a transform-time `this` substitute inside a static
initializer keeps the ContainsLexicalThis transform flag tsc's retained `this` token carries."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/builtins/class_fields/downlevel.rs", [
("""                    Some(match bindings.receiver {
                        StaticReceiver::Bound(binding) => {
                            let identifier = self.create_binding_identifier(&binding)?;
                            if bindings.this_substitution == StaticThisSubstitution::Emit {
                                self.set_original_and_range(identifier, original)?;
                            }
                            identifier.node()
                        }
                        StaticReceiver::InvalidLegacyDecorated => {
                            let value = self.create_void_zero()?;
                            self.create_parenthesized(value)?.node()
                        }
                    })
""", """                    Some(match bindings.receiver {
                        StaticReceiver::Bound(binding) => {
                            let identifier = self.create_binding_identifier(&binding)?;
                            if bindings.this_substitution == StaticThisSubstitution::Emit {
                                self.set_original_and_range(identifier, original)?;
                                // tsc keeps the `this` token in the tree until
                                // print-time substitution (substituteThisExpression),
                                // so its ContainsLexicalThis flag still reaches an
                                // enclosing arrow function (visitArrowFunction →
                                // CapturedLexicalThis → `var _this = this`).
                                self.add_lexical_this_flag(identifier)?;
                            }
                            identifier.node()
                        }
                        StaticReceiver::InvalidLegacyDecorated => {
                            let value = self.create_void_zero()?;
                            let parenthesized = self.create_parenthesized(value)?;
                            // `visitThisExpression` returns the `this` token itself
                            // when the decorated class has no classThis/classConstructor;
                            // `(void 0)` arrives at print time, so the token's
                            // ContainsLexicalThis flag still propagates.
                            self.add_lexical_this_flag(parenthesized)?;
                            parenthesized.node()
                        }
                    })
"""),
("""    fn visit_function_scope(
        &mut self,
        original: TransformNode,
        data: NodeData,
        captures_static_bindings: bool,
    ) -> Result<NodeId, TransformError> {
""", """    /// The transform flag a retained `this` token would carry.
    fn add_lexical_this_flag(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let flags = self.context.arena().transform_flags(node);
        self.context
            .factory()?
            .set_transform_flags(node, flags | TransformFlags::CONTAINS_LEXICAL_THIS);
        Ok(())
    }

    fn visit_function_scope(
        &mut self,
        original: TransformNode,
        data: NodeData,
        captures_static_bindings: bool,
    ) -> Result<NodeId, TransformError> {
"""),
])
# remove the temporary debug hook
p = W + "crates/emitter/src/builtins/es2015.rs"
s = open(p).read()
old = """        if std::env::var_os("TSC_RS_DEBUG_ARROW").is_some() {
            eprintln!(
                "DEBUG visit_arrow_function flags={:?} lexical_this={} facts={:?}",
                self.transform_flags(node),
                self.transform_flags(node).contains(TransformFlags::CONTAINS_LEXICAL_THIS),
                self.print_state.hierarchy_facts
            );
        }
"""
assert s.count(old) == 1
open(p, "w").write(s.replace(old, ""))
print("r7 part 4 applied; debug hook removed")
