use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};
use tsc_types::CompilerOptions;

#[test]
fn routes_parse_order_arenas_without_changing_program_order() {
    // tsc may request the root before its dependency while exposing
    // the dependency first from Program.getSourceFiles().
    let root = parse_source_file(
        "/a.ts",
        r#"import { b } from "./b"; export const a = b;"#,
        ParseOptions::default(),
        None,
    );
    let dependency = parse_source_file(
        "/b.ts",
        "export const b = 1;",
        ParseOptions {
            node_id_base: root.arena.node_end(),
            node_array_id_base: root.arena.array_end(),
            ..ParseOptions::default()
        },
        None,
    );
    let options = CompilerOptions::default();

    let mut dependency_binder = Binder::with_bases(&dependency, &options, 1, 0);
    dependency_binder.bind_source_file();
    let mut root_binder = Binder::with_bases(
        &root,
        &options,
        dependency_binder.next_symbol_id(),
        dependency_binder.symbols.next_id().0,
    );
    root_binder.bind_source_file();

    let program = ProgramBinder::new(vec![&dependency_binder, &root_binder]);
    assert_eq!(
        program
            .files()
            .map(|binder| binder.source.file_name.as_str())
            .collect::<Vec<_>>(),
        ["/b.ts", "/a.ts"]
    );
    assert_eq!(program.source(0).file_name, "/b.ts");
    assert_eq!(program.source(1).file_name, "/a.ts");

    for id in dependency.arena.node_ids() {
        assert_eq!(program.file_index_of_node(id), 0);
        assert!(std::ptr::eq(program.source_of_node(id), &dependency));
    }
    for id in root.arena.node_ids() {
        assert_eq!(program.file_index_of_node(id), 1);
        assert!(std::ptr::eq(program.source_of_node(id), &root));
    }

    for raw in dependency.arena.array_base()..dependency.arena.array_end() {
        let id = NodeArrayId(raw);
        assert!(std::ptr::eq(
            program.node_array(id),
            dependency.arena.node_array(id)
        ));
    }
    for raw in root.arena.array_base()..root.arena.array_end() {
        let id = NodeArrayId(raw);
        assert!(std::ptr::eq(
            program.node_array(id),
            root.arena.node_array(id)
        ));
    }

    for raw in dependency_binder.symbols.base()..dependency_binder.symbols.next_id().0 {
        let id = SymbolId(raw);
        assert!(std::ptr::eq(
            program.symbol(id),
            dependency_binder.symbols.symbol(id)
        ));
    }
    for raw in root_binder.symbols.base()..root_binder.symbols.next_id().0 {
        let id = SymbolId(raw);
        assert!(std::ptr::eq(
            program.symbol(id),
            root_binder.symbols.symbol(id)
        ));
    }
}

#[test]
fn owner_lookup_rejects_ids_outside_every_interval() {
    let owners = [
        ArenaOwner {
            start: 10,
            end: 12,
            file: 1,
        },
        ArenaOwner {
            start: 15,
            end: 18,
            file: 0,
        },
    ];

    assert_eq!(ProgramBinder::owner_file(&owners, 10, "test id"), 1);
    assert_eq!(ProgramBinder::owner_file(&owners, 17, "test id"), 0);
    for id in [9, 12, 14, 18] {
        assert!(
            std::panic::catch_unwind(|| ProgramBinder::owner_file(&owners, id, "test id")).is_err(),
            "id {id} must fail closed"
        );
    }
}
