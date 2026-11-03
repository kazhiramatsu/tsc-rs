use super::*;
use crate::state::CheckerState;
use std::sync::Arc;
use tsc_binder::BinderWorker;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::{parse_published_source_file, ParseOptions};
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
    let worker =
        BinderWorker::bind_in_identity_domain(&source, domain).expect("bind identity allocation");
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
    let domain = tsc_types::IdentityDomain::new();
    let parse = |name: &str, text: &str| {
        tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
            name.to_owned(),
            tsc_diagnostics::TextSnapshot::new(text.to_owned(), Default::default()),
            ParseOptions::default(),
            None,
            &domain,
        )
        .expect("test source identities")
    };
    let root = parse("/a.ts", r#"import { b } from "./b"; export const a = b;"#);
    let dependency = parse("/b.ts", "export const b = 1;");

    let dependency_binder =
        Binder::bind_in_identity_domain(&dependency, &domain).expect("test bind identities");
    let root_binder =
        Binder::bind_in_identity_domain(&root, &domain).expect("test bind identities");

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

    assert_eq!(ProgramBinder::try_owner_file(&owners, 10), Some(1));
    assert_eq!(ProgramBinder::try_owner_file(&owners, 17), Some(0));
    for id in [9, 12, 14, 18] {
        assert_eq!(ProgramBinder::try_owner_file(&owners, id), None);
    }
}

#[test]
fn a_program_view_is_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProgramBinder<'static>>();
}

#[test]
fn try_new_rejects_unpublished_and_cross_domain_programs() {
    let local = tsc_syntax::parse_source_file("/local.ts", "let a = 1;", Default::default(), None);
    let local_binder = tsc_binder::bind_source_file(&local);
    assert!(matches!(
        ProgramBinder::try_new(vec![&local_binder]),
        Err(ProgramIdentityError::PartialIdentityOwnership { file: 0 })
    ));

    let first = parse_published_source_file("/first.ts", "let a = 1;", Default::default(), None);
    let second = parse_published_source_file("/second.ts", "let b = 2;", Default::default(), None);
    let first_binder = tsc_binder::bind_source_file(&first);
    let second_binder = tsc_binder::bind_source_file(&second);
    assert!(matches!(
        ProgramBinder::try_new(vec![&first_binder, &second_binder]),
        Err(ProgramIdentityError::IdentityDomainMismatch { file: 1 })
    ));
}

#[test]
fn snapshot_reuses_owned_handles_across_fresh_checker_sessions() {
    let identity_domain = IdentityDomain::new();
    let mut source = tsc_syntax::parse_source_file(
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
    let worker: BinderWorker<'_> = BinderWorker::bind_in_identity_domain(&source, &identity_domain)
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
    let domain = IdentityDomain::new();
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
    let domain = IdentityDomain::new();
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
    let worker =
        BinderWorker::bind_in_identity_domain(&source, &domain).expect("bind identity allocation");
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
    let address = DocumentAddress::new(&ParseOptions::default(), &domain);
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
fn registry_tells_parse_and_domain_addresses_apart() {
    let registry = DocumentRegistry::new();
    let domain = registry.identity_domain().clone();
    let text = "export const value = 1;";
    let document = bound_document_for_snapshot(
        "/registry.ts",
        TextSnapshot::new(text, DocumentVersion::default()),
        &domain,
    );
    let address = DocumentAddress::new(&ParseOptions::default(), &domain);
    registry.insert(address, &document);

    let other_parse = ParseOptions {
        script_kind: tsc_syntax::ScriptKind::Js,
        ..ParseOptions::default()
    };
    for address in [
        DocumentAddress::new(&other_parse, &domain),
        DocumentAddress::new(&ParseOptions::default(), &IdentityDomain::new()),
    ] {
        assert!(registry
            .get("/registry.ts".into(), text, &address)
            .is_none());
    }
    // Identity bases and compiler options do not take part in the address
    // (parsing and binding read no options, as in tsgo's parse cache key).
    let based = ParseOptions {
        node_id_base: 7,
        node_array_id_base: 3,
        ..ParseOptions::default()
    };
    assert!(registry
        .get(
            "/registry.ts".into(),
            text,
            &DocumentAddress::new(&based, &domain)
        )
        .is_some());
}

/// The binder marks every scope with locals in the node's flag byte, so
/// `locals_of` answers every other node from that byte: the tables it finds
/// are exactly the bound file's.
#[test]
fn locals_are_found_for_exactly_the_scopes_that_have_them() {
    let text = "function f(a: number) { let b = a; { const c = b; } }\n\
                const g = (x: string) => x;\n\
                class K { m() { for (let i = 0; i < 1; i++) {} } }\n";
    crate::state::test_support::with_program_state(
        &[("scopes.ts", text)],
        &CompilerOptions::default(),
        |state| {
            let bound = state.binder.file(0);
            let source = state.binder.source(0);
            let mut scopes = 0;
            for node in source.arena.node_ids() {
                let found = state.binder.locals_of(node).is_some();
                assert_eq!(found, bound.locals.contains_key(&node), "{node:?}");
                scopes += usize::from(found);
            }
            assert!(scopes >= 5, "the source file, functions, blocks and loop");
        },
    );
}
