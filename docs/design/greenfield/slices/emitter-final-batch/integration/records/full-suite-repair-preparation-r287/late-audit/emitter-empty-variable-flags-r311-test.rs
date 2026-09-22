#[test]
fn empty_variable_list_keeps_recovery_and_has_no_binding_pattern_transform_flags() {
    let text = "declare function sink(value: unknown): void;\nconst = 5;\nsink(0);\n";
    for target in [ScriptTarget::ES5, ScriptTarget::ES2015] {
        let parsed = parse_source_file(
            "/project/input.ts",
            text,
            ParseOptions {
                script_target: target,
                ..Default::default()
            },
            None,
        );
        assert_eq!(parsed.parse_diagnostics.len(), 2);
        let diagnostics = parsed.parse_diagnostics.clone();
        let recovery = parsed.parse_recovery().clone();
        let NodeData::SourceFile(data) = &parsed.arena.node(parsed.root).data else {
            panic!("source root");
        };
        let statement = parsed.arena.node_array(data.statements.unwrap()).nodes[1];
        let NodeData::VariableStatement(data) = &parsed.arena.node(statement).data else {
            panic!("empty variable statement");
        };
        let list = data.declaration_list.unwrap();
        let NodeData::VariableDeclarationList(data) = &parsed.arena.node(list).data else {
            panic!("empty declaration list");
        };
        assert!(parsed
            .arena
            .node_array(data.declarations.unwrap())
            .nodes
            .is_empty());
        assert!(
            tsc_types::NodeFlags::from_bits(parsed.arena.node(list).flags)
                .contains(tsc_types::NodeFlags::CONST)
        );
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        initialize_transform_flags(&mut arena, source).unwrap();
        for (label, id) in [
            ("root", parsed.root),
            ("statement", statement),
            ("list", list),
        ] {
            let node = arena.node_ref(source, id).unwrap();
            let flags = arena.transform_flags(node);
            eprintln!("EMPTY_VARIABLE_FLAGS target={target:?} node={label} flags={flags:?}");
            assert!(!flags.contains(TransformFlags::CONTAINS_BINDING_PATTERN));
        }
        let retained = arena.source(source).unwrap().syntax();
        assert_eq!(retained.parse_diagnostics, diagnostics);
        assert_eq!(retained.parse_recovery(), &recovery);
    }
}
