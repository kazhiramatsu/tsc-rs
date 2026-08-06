use super::*;
use crate::state::CheckerState;
use std::sync::Arc;
use tsc_binder::BinderWorker;
use tsc_syntax::{parse_source_file, ParseOptions};
use tsc_types::{CompilerOptions, IdentityDomain};

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
            node_id_base: root.arena.node_end() + 5,
            node_array_id_base: root.arena.array_end() + 3,
            ..ParseOptions::default()
        },
        None,
    );
    let options = CompilerOptions::default();

    let mut dependency_binder = Binder::with_bases(&dependency, &options, 1, 9);
    dependency_binder.bind_source_file();
    let mut root_binder = Binder::with_bases(
        &root,
        &options,
        dependency_binder.next_symbol_id(),
        dependency_binder.symbols.next_id().0 + 7,
    );
    root_binder.bind_source_file();

    let mut program = ProgramBinder::new(vec![&dependency_binder, &root_binder]);
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

    let transient = program.create_symbol(SymbolFlags::PROPERTY, "temporary".to_owned());
    assert_ne!(transient.0 & tsc_types::TRANSIENT_SYMBOL_BIT, 0);
    assert!(program
        .symbol(transient)
        .flags
        .contains(SymbolFlags::TRANSIENT));
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

#[test]
fn try_new_rejects_overlap_and_cross_domain_programs() {
    let first = parse_source_file("/first.ts", "let a = 1;", Default::default(), None);
    let second = parse_source_file("/second.ts", "let b = 2;", Default::default(), None);
    let options = CompilerOptions::default();
    let mut first_binder = Binder::with_bases(&first, &options, 1, 0);
    first_binder.bind_source_file();
    let mut second_binder = Binder::with_bases(&second, &options, 1, 0);
    second_binder.bind_source_file();
    assert!(matches!(
        ProgramBinder::try_new(vec![&first_binder, &second_binder]),
        Err(ProgramIdentityError::Overlap {
            space: ProgramIdentitySpace::Node,
            ..
        })
    ));

    let first_domain = tsc_types::IdentityDomain::reclaiming();
    let second_domain = tsc_types::IdentityDomain::reclaiming();
    let mut first = parse_source_file("/first.ts", "let a = 1;", Default::default(), None);
    first.relocate_into_identity_domain(&first_domain).unwrap();
    let mut second = parse_source_file("/second.ts", "let b = 2;", Default::default(), None);
    second
        .relocate_into_identity_domain(&second_domain)
        .unwrap();
    let first_binder = Binder::bind_in_identity_domain(&first, &options, &first_domain).unwrap();
    let second_binder = Binder::bind_in_identity_domain(&second, &options, &second_domain).unwrap();
    assert!(matches!(
        ProgramBinder::try_new(vec![&first_binder, &second_binder]),
        Err(ProgramIdentityError::IdentityDomainMismatch { file: 1 })
    ));
}

#[test]
fn snapshot_reuses_owned_handles_across_fresh_checker_sessions() {
    let identity_domain = IdentityDomain::reclaiming();
    let mut source = parse_source_file(
        "/snapshot.ts",
        "export const answer: number = 42;",
        ParseOptions::default(),
        None,
    );
    source
        .relocate_into_identity_domain(&identity_domain)
        .expect("source identity allocation");
    let source = Arc::new(source);
    let options = CompilerOptions::default();
    let worker: BinderWorker<'_> =
        BinderWorker::bind_in_identity_domain(&source, &options, &identity_domain)
            .expect("bind identity allocation");
    let data = worker.into_bind_data();
    let parsed = Arc::new(ParsedDocument::new(Arc::clone(&source)));
    let document = Arc::new(BoundDocument::new(parsed, data));
    let snapshot =
        ProgramSnapshot::new(vec![Arc::clone(&document)], 1).expect("snapshot identity allocation");

    let mut first = CheckerState::from_snapshot(&snapshot, &options);
    let mut second = CheckerState::from_snapshot(&snapshot, &options);

    assert!(Arc::ptr_eq(snapshot.document(0), &document));
    assert!(std::ptr::eq(first.binder.source(0), source.as_ref()));
    assert!(std::ptr::eq(second.binder.source(0), source.as_ref()));
    let first_transient = first
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "first".to_owned());
    let second_transient = second
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "second".to_owned());
    assert_eq!(first_transient, second_transient);
    assert!(!std::ptr::eq(
        first.binder.symbol(first_transient),
        second.binder.symbol(second_transient)
    ));
    assert_eq!(
        snapshot.document(0).data.next_symbol_id(),
        document.data.next_symbol_id()
    );
}
