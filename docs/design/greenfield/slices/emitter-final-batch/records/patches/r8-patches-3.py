#!/usr/bin/env python3
"""r8 part 3 (EF6-IMPORT-TYPE-SELF): getContainersOfSymbol's class-expression candidate — a class
expression assigned to `module.exports` / `exports.x` / `<entity>.x` names the source file (or the
resolved receiver) as its container, so the module's export= short-circuit renders `import(".")`."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/checker/src/check.rs", [
("""            if self.kind_of(parent) == SyntaxKind::ModuleBlock {
                if let Some(grandparent) = self.parent_of(parent) {
                    if let Some(module_symbol) = self.node_symbol(grandparent) {
                        if self.resolve_external_module_symbol(Some(module_symbol), false)?
                            == Some(symbol)
                        {
                            candidates.push(module_symbol);
                        }
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
""", """            if self.kind_of(parent) == SyntaxKind::ModuleBlock {
                if let Some(grandparent) = self.parent_of(parent) {
                    if let Some(module_symbol) = self.node_symbol(grandparent) {
                        if self.resolve_external_module_symbol(Some(module_symbol), false)?
                            == Some(symbol)
                        {
                            candidates.push(module_symbol);
                        }
                    }
                }
                continue;
            }
            // `isClassExpression(d) && isBinaryExpression(d.parent) && … =`
            // with an access-expression left over an entity name: a class
            // assigned to `module.exports`/`exports.x` is contained by the
            // source file; any other receiver contributes its resolved
            // symbol (getContainersOfSymbol, _tsc.js:49999-50007).
            if self.kind_of(declaration) == SyntaxKind::ClassExpression {
                if let Some(candidate) = self.class_expression_assignment_container(parent)? {
                    candidates.push(candidate);
                }
            }
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
"""),
("""    fn containers_of_symbol_slice(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: tsc_types::SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>> {
""", """    /// The container of a class expression on the right of
    /// `<access expression> = class …` (getContainersOfSymbol's
    /// class-expression candidate).
    fn class_expression_assignment_container(
        &mut self,
        parent: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        let NodeData::BinaryExpression(binary) = self.data_of(parent) else {
            return Ok(None);
        };
        let (Some(operator), Some(left)) = (binary.operator_token, binary.left) else {
            return Ok(None);
        };
        if self.kind_of(operator) != SyntaxKind::EqualsToken {
            return Ok(None);
        }
        let receiver = match self.data_of(left) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        let Some(receiver) = receiver else {
            return Ok(None);
        };
        let source = self.binder.source_of_node(parent);
        if !node_util::is_entity_name_expression(source, receiver) {
            return Ok(None);
        }
        if tsc_binder::assignment::is_module_exports_access_expression(source, left)
            || tsc_binder::assignment::is_exports_identifier(source, receiver)
        {
            return Ok(self.node_symbol(source.root));
        }
        self.get_resolved_symbol(receiver)
    }

    fn containers_of_symbol_slice(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: tsc_types::SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>> {
"""),
])
print("r8 part 3 applied")
