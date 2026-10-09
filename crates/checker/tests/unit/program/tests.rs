use super::*;
use crate::state::CheckerState;
use std::sync::Arc;
use tsc_binder::BinderWorker;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::{parse_source_file, ParseOptions};
use tsc_types::{CompilerOptions, IdentityDomain};

fn bound_document_for_snapshot(
    path: &str,
    snapshot: Arc<TextSnapshot>,
    domain: &IdentityDomain,
) -> Arc<BoundDocument> {
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        path.to_owned(),
        Arc::clone(&snapshot),
        ParseOptions::default(),
        None,
        domain,
    )
    .expect("source identity allocation");
    let source = Arc::new(source);
    let options = CompilerOptions::default();
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, domain)
        .expect("bind identity allocation");
    let data = worker.into_bind_data();
    Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(source)),
        data,
    ))
}

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
        dependency_binder.symbols.next_id().index() + 7,
    );
    root_binder.bind_source_file();

    let mut program = ProgramBinder::new(vec![&dependency_binder, &root_binder]);
    assert_eq!(
        program
            .files()
            .map(|binder| binder
                .source
                .file_name
                .as_str()
                .expect("scalar filename observation"))
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
        let id = NodeArrayId::new(raw);
        assert!(std::ptr::eq(
            program.node_array(id).nodes,
            dependency.arena.node_array(id).nodes
        ));
    }
    for raw in root.arena.array_base()..root.arena.array_end() {
        let id = NodeArrayId::new(raw);
        assert!(std::ptr::eq(
            program.node_array(id).nodes,
            root.arena.node_array(id).nodes
        ));
    }

    for raw in dependency_binder.symbols.base()..dependency_binder.symbols.next_id().index() {
        let id = SymbolId::new(raw);
        assert!(std::ptr::eq(
            program.symbol(id),
            dependency_binder.symbols.symbol(id)
        ));
    }
    for raw in root_binder.symbols.base()..root_binder.symbols.next_id().index() {
        let id = SymbolId::new(raw);
        assert!(std::ptr::eq(
            program.symbol(id),
            root_binder.symbols.symbol(id)
        ));
    }

    let transient = program.create_symbol(
        SymbolFlags::PROPERTY,
        tsc_types::EscapedName::from_escaped_value(("temporary".to_owned()).into()),
    );
    assert_ne!(transient.index() & tsc_types::TRANSIENT_SYMBOL_BIT, 0);
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
fn owner_hint_preserves_sparse_boundaries_under_shared_queries() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProgramBinder<'static>>();

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
    // An invalid initial hint must fall back to validated interval lookup.
    let hint = AtomicUsize::new(usize::MAX);
    assert_eq!(
        ProgramBinder::try_owner_file_with_hint(&[], 10, &hint),
        None
    );
    let queries = [
        (10, Some(1)),
        (17, Some(0)),
        (9, None),
        (11, Some(1)),
        (12, None),
        (15, Some(0)),
        (14, None),
        (18, None),
        (u32::MAX, None),
    ];
    std::thread::scope(|scope| {
        for offset in 0..2 {
            let owners = &owners;
            let hint = &hint;
            let queries = &queries;
            scope.spawn(move || {
                for round in 0..128 {
                    for step in 0..queries.len() {
                        let (id, expected) = queries[(round + step + offset) % queries.len()];
                        assert_eq!(
                            ProgramBinder::try_owner_file_with_hint(owners, id, hint),
                            expected,
                            "id {id} must retain its owner or be rejected"
                        );
                    }
                }
            });
        }
    });
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
    let first_transient = first.binder.create_symbol(
        SymbolFlags::PROPERTY,
        tsc_types::EscapedName::from_escaped_value(("first".to_owned()).into()),
    );
    let second_transient = second.binder.create_symbol(
        SymbolFlags::PROPERTY,
        tsc_types::EscapedName::from_escaped_value(("second".to_owned()).into()),
    );
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

#[test]
fn program_membership_can_change_without_mutating_a_shared_document() {
    let domain = IdentityDomain::reclaiming();
    let text = TextSnapshot::new(
        "interface SharedDeclaration { value: string }\n",
        DocumentVersion::new("shared"),
    );
    let document = bound_document_for_snapshot("/shared.d.ts", text, &domain);
    let default_library = ProgramSnapshot::new_with_file_facts(
        vec![Arc::clone(&document)],
        vec![ProgramFileFacts::DEFAULT_LIBRARY],
    )
    .expect("default-library Program");
    let ordinary_source = ProgramSnapshot::new_with_file_facts(
        vec![Arc::clone(&document)],
        vec![ProgramFileFacts::ORDINARY],
    )
    .expect("ordinary-source Program");
    let options = CompilerOptions {
        skip_default_lib_check: Some(true),
        ..CompilerOptions::default()
    };
    let default_state = CheckerState::from_snapshot(&default_library, &options);
    let ordinary_state = CheckerState::from_snapshot(&ordinary_source, &options);
    let file = ProgramFileId::from_raw(0);

    assert!(Arc::ptr_eq(
        default_library.document(0),
        ordinary_source.document(0),
    ));
    assert!(default_state.skip_type_checking_file(file));
    assert!(!ordinary_state.skip_type_checking_file(file));
}

#[test]
fn ephemeral_store_publishes_only_completed_owned_documents() {
    let domain = IdentityDomain::reclaiming();
    let snapshot = TextSnapshot::new("export const value = 1;", DocumentVersion::new("1"));
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/ephemeral.ts".to_owned(),
        Arc::clone(&snapshot),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("source identity allocation");
    let source = Arc::new(source);
    let options = CompilerOptions::default();
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("bind identity allocation");
    let mut store = EphemeralDocumentStore::new(domain.clone());
    let document = store
        .publish(Arc::clone(&source), worker.into_bind_data())
        .expect("completed bind belongs to the ephemeral store domain");

    assert_eq!(store.documents().len(), 1);
    assert!(Arc::ptr_eq(document.source().snapshot(), &snapshot));
    let program = store
        .into_snapshot(0)
        .expect("ephemeral store publishes a valid ProgramSnapshot");
    assert_eq!(program.file_count(), 1);
    assert!(Arc::ptr_eq(program.document(0), &document));
}
#[test]
fn registry_shares_a_document_while_a_program_holds_it() {
    // tsgo's parse cache: the same file, text and address find the document
    // a Program holds; once no Program holds it, nothing is found.
    let registry = DocumentRegistry::new();
    let domain = registry.identity_domain().clone();
    let text = "export const value = 1;";
    let document = bound_document_for_snapshot(
        "/registry.ts",
        TextSnapshot::new(text, DocumentVersion::default()),
        &domain,
    );
    let address = DocumentAddress::new(
        &ParseOptions::default(),
        CompilerOptions::default(),
        &domain,
    );
    registry.insert(address.clone(), &document);
    assert_eq!(registry.len(), 1);

    let found = registry
        .get("/registry.ts".into(), text, &address)
        .expect("the held document");
    assert!(Arc::ptr_eq(&found, &document));
    assert!(registry
        .get("/registry.ts".into(), "export const value = 2;", &address)
        .is_none());
    assert!(registry.get("/other.ts".into(), text, &address).is_none());

    drop(found);
    drop(document);
    assert!(registry
        .get("/registry.ts".into(), text, &address)
        .is_none());
    assert_eq!(registry.len(), 0);
    registry.purge();
    assert!(registry.is_empty());
}

#[test]
fn registry_tells_parse_bind_and_domain_addresses_apart() {
    let registry = DocumentRegistry::new();
    let domain = registry.identity_domain().clone();
    let text = "export const value = 1;";
    let document = bound_document_for_snapshot(
        "/registry.ts",
        TextSnapshot::new(text, DocumentVersion::default()),
        &domain,
    );
    let address = DocumentAddress::new(
        &ParseOptions::default(),
        CompilerOptions::default(),
        &domain,
    );
    registry.insert(address, &document);

    let other_parse = ParseOptions {
        javascript_file: true,
        ..ParseOptions::default()
    };
    let other_bind = CompilerOptions {
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    for address in [
        DocumentAddress::new(&other_parse, CompilerOptions::default(), &domain),
        DocumentAddress::new(&ParseOptions::default(), other_bind, &domain),
        DocumentAddress::new(
            &ParseOptions::default(),
            CompilerOptions::default(),
            &IdentityDomain::reclaiming(),
        ),
    ] {
        assert!(registry
            .get("/registry.ts".into(), text, &address)
            .is_none());
    }
    // Identity bases do not take part in the address.
    let based = ParseOptions {
        node_id_base: 7,
        node_array_id_base: 3,
        ..ParseOptions::default()
    };
    assert!(registry
        .get(
            "/registry.ts".into(),
            text,
            &DocumentAddress::new(&based, CompilerOptions::default(), &domain)
        )
        .is_some());
}
