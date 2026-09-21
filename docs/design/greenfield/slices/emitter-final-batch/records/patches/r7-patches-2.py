#!/usr/bin/env python3
"""r7 part 2: EF5-BARE-CLONE-SPELLING (cloneNode alone prints idText; only the getName
adaptation keeps source spelling) and EF2-ARROW-BLOCK-ORDER (convertToFunctionBlock runs on the
unparenthesized concise body)."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/factory.rs", [
("""        self.arena.copy_literal_properties(original, clone);
        if matches!(
            record.data,
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_)
        ) {
            self.arena.metadata_mut(clone).cloned_identifier_spelling = true;
        }
        Ok(clone)
    }
""", """        self.arena.copy_literal_properties(original, clone);
        Ok(clone)
    }

    /// `setParent(setTextRange(cloneNode(name), name), name.parent)` — the
    /// `getName` family (_tsc.js:24788-24799) threads the parsed name's range
    /// and parent through the clone, so `getTextOfNode` prints it from the
    /// source text (`\\u0046oo` keeps its escape). A bare `cloneNode(name)`
    /// (`createExportExpression`, the generators' hoisted names) stays
    /// synthetic and prints `idText`. Adaptation sites that keep the clone
    /// position-synthetic for other reasons request the source spelling
    /// through this constructor instead of threading the range.
    pub fn clone_node_with_source_spelling(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let clone = self.clone_node(original)?;
        if matches!(
            self.arena.node(clone)?.data,
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_)
        ) {
            self.arena.metadata_mut(clone).cloned_identifier_spelling = true;
        }
        Ok(clone)
    }
"""),
])
patch(W + "crates/emitter/src/builtins/es2015.rs", [
("""            let name = name.expect("plain identifier name");
            let clone = self.clone_node(name)?;
            // `setTextRange(cloneNode(nodeName), nodeName)` — the range
""", """            let name = name.expect("plain identifier name");
            let clone = self
                .context
                .factory()?
                .clone_node_with_source_spelling(name)?;
            // `setTextRange(cloneNode(nodeName), nodeName)` — the range
"""),
])
patch(W + "crates/emitter/src/builtins/class_fields/downlevel.rs", [
("""        } else if record.kind == SyntaxKind::ArrowFunction {
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
""", """        } else if record.kind == SyntaxKind::ArrowFunction {
            // `visitFunctionBody` runs `convertToFunctionBlock` on the visited
            // concise body BEFORE `updateArrowFunction` applies
            // `parenthesizeConciseBodyOfArrowFunction`, so a hoisted comma
            // sequence returns unparenthesized; this port updates first, so
            // drop the parentheses that update introduced.
            let body = self.strip_update_introduced_concise_parentheses(function, body)?;
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
"""),
("""    fn install_function_bindings(
        &mut self,
        function: TransformNode,
        bindings: ClassGeneratedBindings,
        initialization_statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
""", """    /// The parenthesized concise body an update introduced (the parsed arrow
    /// body was not parenthesized) unwraps to the expression
    /// `convertToFunctionBlock` would have received upstream.
    fn strip_update_introduced_concise_parentheses(
        &self,
        function: TransformNode,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let arena = self.context.arena();
        let NodeData::ParenthesizedExpression(data) = &arena.node(body)?.data else {
            return Ok(body);
        };
        let original = arena.get_original_node(function);
        let original_body = match &arena.node(original)?.data {
            NodeData::ArrowFunction(data) => data.body,
            _ => None,
        };
        let original_is_parenthesized = match original_body
            .and_then(|original_body| arena.node_ref(original.source(), original_body))
        {
            Some(original_body) => {
                arena.node(original_body)?.kind == SyntaxKind::ParenthesizedExpression
            }
            None => false,
        };
        if original_is_parenthesized {
            return Ok(body);
        }
        Ok(data
            .expression
            .and_then(|expression| arena.node_ref(body.source(), expression))
            .unwrap_or(body))
    }

    fn install_function_bindings(
        &mut self,
        function: TransformNode,
        bindings: ClassGeneratedBindings,
        initialization_statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
"""),
])
print("r7 part 2 applied")
