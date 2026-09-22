#!/usr/bin/env python3
"""r5: ES2018 async-generator super gate; ES2015 assigned-name tracking through synthesized parents; retire 2 EF2 rows."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:90])
        s = s.replace(old, new)
    open(path, "w").write(s)

# (a) ES2018: emitSuperHelpers needs languageVersion >= ES2015 (_tsc.js:102657).
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2018.rs", [
("""        let owns_super_capture = mode.is_async_generator();
        let introduces_super_boundary = owns_super_capture || kind != SyntaxKind::ArrowFunction;
""", """        // `emitSuperHelpers` (_tsc.js:102657): below ES2015 the ES2015 pass
        // lowers `super` itself, so the async generator keeps its direct uses.
        let owns_super_capture =
            mode.is_async_generator() && self.target >= ScriptTarget::ES2015;
        let introduces_super_boundary = owns_super_capture || kind != SyntaxKind::ArrowFunction;
"""),
])

# (b) ES2015: getAssignedName observes the declaration a synthesized class expression
# was placed in (the legacy-decorator `let default_1 = class {}`), carried by the visitor.
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2015.rs", [
("""        let name = name.ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?;
        let updated = if self.is_binding_pattern(name)? {
""", """        let name = name.ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?;
        let initializer = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "variable declaration",
                ));
            };
            data.initializer
        };
        // getAssignedName reads `node.parent`; a synthesized declaration from
        // an earlier pass has no arena parent, so the visitor carries the
        // declaration name for its own initializer (E-NAMES-BASE R9 lineage).
        let saved_assigned_name = self
            .pending_assigned_name
            .replace((initializer, name));
        let updated = if self.is_binding_pattern(name)? {
"""),
("""        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitLabeledStatement @6.0.3
""", """        self.pending_assigned_name = saved_assigned_name;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitLabeledStatement @6.0.3
"""),
("""        let name = match name {
            Some(name) => Some(name),
            None => self.assigned_name(node)?,
        };
""", """        let name = match name {
            Some(name) => Some(name),
            None => match self.assigned_name(node)? {
                Some(name) => Some(name),
                None => self
                    .pending_assigned_name
                    .filter(|(initializer, _)| *initializer == Some(node.node()))
                    .map(|(_, name)| name),
            },
        };
"""),
])
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2015.rs", [
("""    print_state: &'state mut Es2015PrintState,

    /// `currentText` (`skipTrivia` for the class-wrapper end positions;
""", """    print_state: &'state mut Es2015PrintState,

    /// The variable declaration (initializer, name) being visited, so
    /// `getAssignedName` can observe a synthesized parent that has no arena
    /// parent link (a legacy-decorated `let default_1 = class {}`).
    pending_assigned_name: Option<(Option<NodeId>, TransformNode)>,

    /// `currentText` (`skipTrivia` for the class-wrapper end positions;
"""),
("""            converted_loop_state: None,
""", """            converted_loop_state: None,
            pending_assigned_name: None,
"""),
])
print("visitor field + init added")

# (c) retire two rows.
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/compiler/tests/emitter_final_rows.rs"
s = open(p).read()
for row in ["typescript-6.0.3/compiler/emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5.ts#target%3Des5",
            "typescript-6.0.3/conformance/async/es5/asyncAwait_es5.ts#target%3Des5"]:
    head, rest = s.split("const KNOWN: &[&str] = &[", 1)
    known, tail = rest.split("];", 1)
    lines = [l for l in known.split("\n") if l.strip()]
    kept = [l for l in lines if f'"{row}"' not in l]
    assert len(lines) - len(kept) == 1, row
    s = head + "const KNOWN: &[&str] = &[\n" + "\n".join(kept) + "\n];" + tail
s = s.replace("/// sourceMapWithCaseSensitiveFileNamesAndOutDir (EF3-HARNESS-FLOOR).",
 "/// sourceMapWithCaseSensitiveFileNamesAndOutDir (EF3-HARNESS-FLOOR);\n/// emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5\n/// (EF2-ARROW-PARENS); asyncAwait_es5 (EF2-PROMISE-CTOR).")
open(p, "w").write(s)
print("r5 patches applied")
