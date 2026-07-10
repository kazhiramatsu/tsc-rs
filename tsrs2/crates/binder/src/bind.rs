//! The bind walk spine: bind / bindEach / bindEachChild /
//! bindEachFunctionsFirst and the bindChildren dispatch. Stage 3.3
//! lands the structural walk; the flow-aware bindChildren arms are
//! stage 3.5 and bindWorker's per-kind symbol arms are stage 3.4.

use crate::containers::{get_container_flags, ContainerFlags};
use crate::declare::Binder;
use crate::node_util::{kind_of, statements_of};
use tsrs2_syntax::{for_each_child, NodeArrayId, NodeData, NodeId, SyntaxKind};

impl<'a> Binder<'a> {
    /// tsc-port: bind @6.0.3
    /// tsc-hash: d0f56450cb1e141f74a40208f49a2952dd81d23e726f1c07e9d897b22a56f546
    /// tsc-span: _tsc.js:44226-44255
    ///
    /// setParent is unnecessary (arena parents are finalized at parse
    /// time); bindJSDoc awaits JSDoc parsing.
    pub fn bind(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let save_in_strict_mode = self.in_strict_mode;
        self.bind_worker(node);
        if kind_of(self.source, node) as u16 > SyntaxKind::LastToken as u16 {
            let container_flags = get_container_flags(self.source, node);
            if container_flags == ContainerFlags::NONE {
                self.bind_children(node);
            } else {
                self.bind_container(node, container_flags);
            }
        }
        self.in_strict_mode = save_in_strict_mode;
    }

    /// bindWorker (44287): the per-kind symbol arms land in stage 3.4.
    pub(crate) fn bind_worker(&mut self, node: NodeId) {
        let _ = node;
    }

    /// tsc bindEach (42834). Consumed by the stage-3.5 flow binders.
    #[allow(dead_code)]
    fn bind_each(&mut self, nodes: Option<NodeArrayId>) {
        let Some(nodes) = nodes else { return };
        let nodes = self.source.arena.node_array(nodes).nodes.clone();
        for node in nodes {
            self.bind(Some(node));
        }
    }

    /// tsc-port: bindEachFunctionsFirst @6.0.3
    /// tsc-hash: 43522e842ac4d5d4a7b9d6e6a18bc582366d484bca16403e793fad540f52077d
    /// tsc-span: _tsc.js:42830-42833
    ///
    /// OBSERVABLE binding order: FunctionDeclarations bind before the
    /// other statements (hoisting), which shows up in declaration order
    /// and duplicate-diagnostic order.
    fn bind_each_functions_first(&mut self, nodes: Option<NodeArrayId>) {
        let Some(nodes) = nodes else { return };
        let nodes = self.source.arena.node_array(nodes).nodes.clone();
        for &node in &nodes {
            if kind_of(self.source, node) == SyntaxKind::FunctionDeclaration {
                self.bind(Some(node));
            }
        }
        for &node in &nodes {
            if kind_of(self.source, node) != SyntaxKind::FunctionDeclaration {
                self.bind(Some(node));
            }
        }
    }

    /// tsc bindEachChild (42840): every child in forEachChild order.
    fn bind_each_child(&mut self, node: NodeId) {
        let mut children = Vec::new();
        for_each_child(&self.source.arena, self.source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            self.bind(Some(child));
        }
    }

    /// bindChildren (42843): stage 3.3 carries the structural dispatch
    /// (functions-first statement lists, inAssignmentPattern
    /// save/restore); the flow-aware statement/expression arms and the
    /// unreachable stamping are stage 3.5.
    pub(crate) fn bind_children(&mut self, node: NodeId) {
        let save_in_assignment_pattern = self.in_assignment_pattern;
        self.in_assignment_pattern = false;
        match kind_of(self.source, node) {
            SyntaxKind::SourceFile => {
                let (statements, end_of_file_token) = match &self.source.arena.node(node).data {
                    NodeData::SourceFile(data) => (data.statements, data.end_of_file_token),
                    _ => (None, None),
                };
                self.bind_each_functions_first(statements);
                self.bind(end_of_file_token);
            }
            SyntaxKind::Block | SyntaxKind::ModuleBlock => {
                self.bind_each_functions_first(statements_of(self.source, node));
            }
            SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::SpreadElement => {
                self.in_assignment_pattern = save_in_assignment_pattern;
                self.bind_each_child(node);
            }
            _ => {
                self.bind_each_child(node);
            }
        }
        self.in_assignment_pattern = save_in_assignment_pattern;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::{get_module_instance_state, ModuleInstanceState};
    use tsrs2_syntax::{parse_source_file, ParseOptions, SourceFile};
    use tsrs2_types::NodeFlags;

    fn parse(text: &str) -> SourceFile {
        parse_source_file("main.ts", text, ParseOptions::default(), None)
    }

    fn find_nodes(source: &SourceFile, kind: SyntaxKind) -> Vec<NodeId> {
        (0..source.arena.len() as u32)
            .map(NodeId)
            .filter(|&id| source.arena.node(id).kind == kind)
            .collect()
    }

    #[test]
    fn container_flags_table_pins() {
        let source = parse(
            "function f() { { let a; } }\n\
             const g = function() {};\n\
             const h = () => 1;\n\
             const o = { m() {} };\n\
             class C { m() {} constructor() {} }\n\
             interface I { m(): void }\n\
             namespace N { }\n\
             type T = { a: string };\n",
        );
        let flags = |id: NodeId| get_container_flags(&source, id);
        assert_eq!(flags(source.root).0, 1 | 4 | 32);
        let function = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        assert_eq!(flags(function).0, 1 | 4 | 32 | 8);
        let function_expression = find_nodes(&source, SyntaxKind::FunctionExpression)[0];
        assert_eq!(flags(function_expression).0, 1 | 4 | 32 | 8 | 16);
        let arrow = find_nodes(&source, SyntaxKind::ArrowFunction)[0];
        assert_eq!(flags(arrow).0, 1 | 4 | 32 | 8 | 16 | 256);
        // Object-literal method gains flag 128; a class-DECLARATION
        // method does not.
        let methods = find_nodes(&source, SyntaxKind::MethodDeclaration);
        assert_eq!(flags(methods[0]).0, 1 | 4 | 32 | 8 | 128);
        assert_eq!(flags(methods[1]).0, 1 | 4 | 32 | 8);
        let constructor = find_nodes(&source, SyntaxKind::Constructor)[0];
        assert_eq!(flags(constructor).0, 1 | 4 | 32 | 8);
        let interface = find_nodes(&source, SyntaxKind::InterfaceDeclaration)[0];
        assert_eq!(flags(interface).0, 1 | 64);
        let module = find_nodes(&source, SyntaxKind::ModuleDeclaration)[0];
        assert_eq!(flags(module).0, 1 | 32);
        let method_signature = find_nodes(&source, SyntaxKind::MethodSignature)[0];
        assert_eq!(flags(method_signature).0, 1 | 4 | 32 | 8 | 256);
        // A function body block is NOT a block-scoped container; a free
        // block is. (Arena order is bottom-up: the inner free block
        // finishes before the enclosing function body.)
        let blocks = find_nodes(&source, SyntaxKind::Block);
        assert_eq!(flags(blocks[0]).0, 2 | 32);
        assert_eq!(flags(blocks[1]).0, 0);
    }

    #[test]
    fn property_declaration_is_flow_container_only_with_initializer() {
        let source = parse("class C { a = 1; b: string; }\n");
        let properties = find_nodes(&source, SyntaxKind::PropertyDeclaration);
        assert_eq!(get_container_flags(&source, properties[0]).0, 4);
        assert_eq!(get_container_flags(&source, properties[1]).0, 0);
    }

    #[test]
    fn bind_walk_builds_scope_tree_and_stamps_implicit_return() {
        let source = parse("function f() { var x; }\nfunction g();\n");
        let mut binder = Binder::new(&source);
        binder.container = None;
        binder.bind(Some(source.root));
        let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        let g = find_nodes(&source, SyntaxKind::FunctionDeclaration)[1];
        // HasLocals containers got (empty) locals tables + chain order.
        assert!(binder.locals.contains_key(&source.root));
        assert!(binder.locals.contains_key(&f));
        assert_eq!(binder.next_container.get(&source.root), Some(&f));
        // Reachable function body ⇒ HasImplicitReturn + endFlowNode;
        // a bodyless overload signature gets neither.
        assert!(binder.flags_of(f).intersects(NodeFlags::HAS_IMPLICIT_RETURN));
        assert!(binder.node_end_flow.contains_key(&f));
        assert!(!binder.flags_of(g).intersects(NodeFlags::HAS_IMPLICIT_RETURN));
        assert!(!binder.node_end_flow.contains_key(&g));
        // SourceFile endFlowNode always stamps.
        assert!(binder.node_end_flow.contains_key(&source.root));
    }

    #[test]
    fn iife_skips_fresh_start_flow_and_gets_return_target() {
        let source = parse("(function() { var a; })();\nconst x = function() {};\n");
        let mut binder = Binder::new(&source);
        binder.bind(Some(source.root));
        let functions = find_nodes(&source, SyntaxKind::FunctionExpression);
        let (iife, plain) = (functions[0], functions[1]);
        // The IIFE inherits the enclosing flow: no Start node carries it
        // as payload. The plain function expression gets its own Start.
        let start_payloads: Vec<_> = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| {
                binder
                    .flow
                    .flow(id)
                    .flags
                    .intersects(tsrs2_types::FlowFlags::START)
            })
            .map(|id| binder.flow.flow(id).payload.clone())
            .collect();
        assert!(!start_payloads.contains(&crate::flow::FlowPayload::Node(iife)));
        assert!(start_payloads.contains(&crate::flow::FlowPayload::Node(plain)));
        // Both still report an implicit return (bodies are reachable).
        assert!(binder
            .flags_of(iife)
            .intersects(NodeFlags::HAS_IMPLICIT_RETURN));
    }

    #[test]
    fn module_instance_state_pins() {
        let source = parse(
            "namespace A { interface I {} type T = I; }\n\
             namespace B { const enum E { X } }\n\
             namespace C { var v: number; }\n\
             namespace D { export { I2 }; interface I2 {} }\n\
             interface I2 {}\n",
        );
        let modules = find_nodes(&source, SyntaxKind::ModuleDeclaration);
        let mut state =
            |id: NodeId| get_module_instance_state(&source, id, &mut std::collections::HashMap::new());
        assert_eq!(state(modules[0]), ModuleInstanceState::NonInstantiated);
        assert_eq!(state(modules[1]), ModuleInstanceState::ConstEnumOnly);
        assert_eq!(state(modules[2]), ModuleInstanceState::Instantiated);
        // export { I2 } resolves the alias target inside the block:
        // interface ⇒ NonInstantiated.
        assert_eq!(state(modules[3]), ModuleInstanceState::NonInstantiated);
    }

    #[test]
    fn ambient_module_pattern_and_export_modifier_diagnostics() {
        // Pins from tsc bindDiagnostics:
        //   'declare module "a*b*c" {}'      -> 5061 @ (15,7)
        //   'export declare module "m" {}'   -> 2668 @ (0,6)
        for (text, code, start, length) in [
            ("declare module \"a*b*c\" {}\n", 5061u32, 15u32, 7u32),
            ("export declare module \"m\" {}\n", 2668, 0, 6),
        ] {
            let source = parse(text);
            let mut binder = Binder::new(&source);
            binder.container = Some(source.root);
            let module = find_nodes(&source, SyntaxKind::ModuleDeclaration)[0];
            binder.bind_module_declaration(module);
            let pins: Vec<(u32, u32, u32)> = binder
                .bind_diagnostics
                .iter()
                .map(|d| (d.code(), d.start.unwrap(), d.length.unwrap()))
                .collect();
            assert_eq!(pins, [(code, start, length)], "case: {text}");
        }
        // A single star is a valid pattern and lands in
        // patternAmbientModules.
        let source = parse("declare module \"good*\" {}\n");
        let mut binder = Binder::new(&source);
        binder.container = Some(source.root);
        let module = find_nodes(&source, SyntaxKind::ModuleDeclaration)[0];
        binder.bind_module_declaration(module);
        assert!(binder.bind_diagnostics.is_empty());
        assert_eq!(binder.pattern_ambient_modules.len(), 1);
        assert_eq!(binder.pattern_ambient_modules[0].0, "good");
        assert_eq!(binder.pattern_ambient_modules[0].1, "");
    }

    #[test]
    fn declare_module_member_links_local_and_export_symbols() {
        let source = parse("export function f() {}\nfunction local() {}\n");
        let mut binder = Binder::new(&source);
        binder.container = Some(source.root);
        binder.locals.insert(source.root, Default::default());
        let file_symbol = binder
            .symbols
            .alloc(tsrs2_types::SymbolFlags::NONE, "\"main\"".to_owned());
        binder.node_symbol.insert(source.root, file_symbol);
        let functions = find_nodes(&source, SyntaxKind::FunctionDeclaration);

        let exported = binder.declare_module_member(
            functions[0],
            tsrs2_types::SymbolFlags::FUNCTION,
            tsrs2_types::SymbolFlags::FUNCTION_EXCLUDES,
        );
        // Exported: a LOCAL ExportValue symbol linked to the EXPORT
        // symbol; declareModuleMember returns the local.
        let local_symbol = binder.locals[&source.root]["f"];
        assert_eq!(exported, local_symbol);
        let export_symbol = binder.symbols.symbol(local_symbol).export_symbol.unwrap();
        assert_eq!(
            binder.symbols.symbol(file_symbol).exports.get("f"),
            Some(&export_symbol)
        );
        assert!(binder
            .symbols
            .symbol(local_symbol)
            .flags
            .intersects(tsrs2_types::SymbolFlags::EXPORT_VALUE));
        assert_eq!(binder.node_local_symbol.get(&functions[0]), Some(&local_symbol));

        // Unexported: locals only.
        binder.declare_module_member(
            functions[1],
            tsrs2_types::SymbolFlags::FUNCTION,
            tsrs2_types::SymbolFlags::FUNCTION_EXCLUDES,
        );
        assert!(binder.locals[&source.root].contains_key("local"));
        assert!(!binder
            .symbols
            .symbol(file_symbol)
            .exports
            .contains_key("local"));
    }
}
