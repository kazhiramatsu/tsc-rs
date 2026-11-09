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
    let mut builder = DocumentBuilder::new(source);
    let worker = BinderWorker::bind_in_identity_domain(builder.source_mut(), domain)
        .expect("bind identity allocation");
    let data = worker.into_bind_data();
    Arc::new(builder.freeze(data))
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
    let mut root = parse("/a.ts", r#"import { b } from "./b"; export const a = b;"#);
    let mut dependency = parse("/b.ts", "export const b = 1;");

    let dependency_binder =
        Binder::bind_in_identity_domain(&mut dependency, &domain).expect("test bind identities");
    let root_binder =
        Binder::bind_in_identity_domain(&mut root, &domain).expect("test bind identities");

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

    for id in dependency_binder.source.arena.node_ids() {
        assert_eq!(program.file_index_of_node(id), 0);
        assert!(std::ptr::eq(
            program.source_of_node(id),
            dependency_binder.source
        ));
    }
    for id in root_binder.source.arena.node_ids() {
        assert_eq!(program.file_index_of_node(id), 1);
        assert!(std::ptr::eq(program.source_of_node(id), root_binder.source));
    }

    for raw in
        dependency_binder.source.arena.array_base()..dependency_binder.source.arena.array_end()
    {
        let id = NodeArrayId::new(raw);
        assert!(std::ptr::eq(
            program.node_array(id).nodes,
            dependency_binder.source.arena.node_array(id).nodes
        ));
    }
    for raw in root_binder.source.arena.array_base()..root_binder.source.arena.array_end() {
        let id = NodeArrayId::new(raw);
        assert!(std::ptr::eq(
            program.node_array(id).nodes,
            root_binder.source.arena.node_array(id).nodes
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
    let mut local =
        tsc_syntax::parse_source_file("/local.ts", "let a = 1;", Default::default(), None);
    let local_binder = tsc_binder::bind_source_file(&mut local);
    assert!(matches!(
        ProgramBinder::try_new(vec![&local_binder]),
        Err(ProgramIdentityError::PartialIdentityOwnership { file: 0 })
    ));

    let mut first =
        parse_published_source_file("/first.ts", "let a = 1;", Default::default(), None);
    let mut second =
        parse_published_source_file("/second.ts", "let b = 2;", Default::default(), None);
    let first_binder = tsc_binder::bind_source_file(&mut first);
    let second_binder = tsc_binder::bind_source_file(&mut second);
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
    let mut source = Arc::new(source);
    let options = CompilerOptions::default();
    let worker: BinderWorker<'_> = BinderWorker::bind_in_identity_domain(
        Arc::get_mut(&mut source).expect("unshared test source"),
        &identity_domain,
    )
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
    let mut builder = DocumentBuilder::new(source);
    let worker = BinderWorker::bind_in_identity_domain(builder.source_mut(), &domain)
        .expect("bind identity allocation");
    let data = worker.into_bind_data();
    let mut store = EphemeralDocumentStore::new(domain.clone());
    let document = store
        .publish_builder(builder, data)
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
        DocumentAddress::new(&ParseOptions::default(), &domain)
            .with_script_kind(tsc_syntax::ScriptKind::Json),
        DocumentAddress::new(
            &ParseOptions {
                js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseForTypeErrors,
                ..ParseOptions::default()
            },
            &domain,
        ),
        DocumentAddress::new(
            &ParseOptions {
                force_external_module: true,
                ..ParseOptions::default()
            },
            &domain,
        ),
        DocumentAddress::new(
            &ParseOptions {
                detect_external_module_from_jsx: true,
                ..ParseOptions::default()
            },
            &domain,
        ),
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

#[test]
fn json_program_reuses_only_the_actual_json_grammar() {
    let registry = DocumentRegistry::new();
    let domain = registry.identity_domain();
    let path = "/grammar.json";
    let text = r#"{ "value": 1 }"#;
    let json = tsc_syntax::parse_json_text_from_snapshot_in_identity_domain(
        path,
        TextSnapshot::new(text, Default::default()),
        domain,
    )
    .expect("JSON source identities");
    let mut builder = DocumentBuilder::new(json);
    let data = BinderWorker::bind_in_identity_domain(builder.source_mut(), domain)
        .expect("JSON bind identities")
        .into_bind_data();
    let json = Arc::new(builder.freeze(data));
    let typescript =
        bound_document_for_snapshot(path, TextSnapshot::new(text, Default::default()), domain);
    assert_eq!(json.source().script_kind, tsc_syntax::ScriptKind::Json);
    assert_eq!(typescript.source().script_kind, tsc_syntax::ScriptKind::Ts);
    // Both entry points retain the same legacy parse-option shape.
    let options = ParseOptions::default();
    let ts_address = DocumentAddress::new(&options, domain);
    let json_address = ts_address
        .clone()
        .with_script_kind(tsc_syntax::ScriptKind::Json);
    registry.insert(ts_address.clone(), &typescript);
    assert!(registry.get(path.into(), text, &json_address).is_none());
    registry.insert(json_address, &json);
    // The loader still uses the legacy parse-option comparison: it finds the
    // explicit TS parse, and does not gain reuse of JSON-shaped options here.
    let loader_ts = tsc_program::ParseCache::find(&registry, path.into(), text, &options)
        .expect("the loader's existing TS parse match");
    assert!(std::ptr::eq(loader_ts.source(), typescript.source()));
    assert!(tsc_program::ParseCache::find(
        &registry,
        path.into(),
        text,
        &ParseOptions {
            script_kind: tsc_syntax::ScriptKind::Json,
            ..ParseOptions::default()
        },
    )
    .is_none());
    let parsed = crate::parse_program_inputs(
        &[],
        &[crate::InputFile::new(path, text)],
        0,
        &CompilerOptions::default(),
        "/".into(),
        domain,
        None,
        &mut crate::CheckWorkCounters::default(),
        crate::WorkerBudget::serial(),
        Some(&registry),
    );
    assert_eq!(parsed.program_sources.len(), 1);
    match &parsed.program_sources[0] {
        crate::ProgramSource::Shared(found) => assert!(Arc::ptr_eq(found, &json)),
        _ => panic!("the existing JSON document must be reused"),
    }
    assert!(Arc::ptr_eq(
        &registry
            .get(path.into(), text, &ts_address)
            .expect("TS variant"),
        &typescript,
    ));
}

#[test]
fn builder_binds_uniquely_and_freezes_without_copying_the_tree() {
    let domain = IdentityDomain::new();
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/builder.ts",
        TextSnapshot::new("export const value = 1;", DocumentVersion::default()),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("source identities");
    let mut builder = DocumentBuilder::new(source);
    let nodes_address = builder.source().arena.nodes().as_ptr();
    let text = Arc::clone(builder.source().snapshot());
    let root = builder.source().root;
    let worker = BinderWorker::bind_in_identity_domain(builder.source_mut(), &domain)
        .expect("bind identities");
    let data = worker.into_bind_data();
    let mut store = EphemeralDocumentStore::new(domain);
    let document = store.publish_builder(builder, data).expect("freeze");
    assert_eq!(nodes_address, document.source().arena.nodes().as_ptr());
    assert!(Arc::ptr_eq(&text, document.source().snapshot()));
    assert_eq!(root, document.source().root);
    assert!(document.data.node_symbol.get(&root).is_some());
    assert!(Arc::ptr_eq(&document, &store.documents()[0]));
}

#[test]
fn registry_document_keeps_its_leases_at_a_different_program_index() {
    for release_first in [true, false] {
        let registry = DocumentRegistry::new();
        let domain = registry.identity_domain();
        let text = "export const a = 1;";
        let a = bound_document_for_snapshot(
            "/a.ts",
            TextSnapshot::new(text, Default::default()),
            domain,
        );
        let b = bound_document_for_snapshot(
            "/b.ts",
            TextSnapshot::new("export const b = 2;", Default::default()),
            domain,
        );
        let address = DocumentAddress::new(&ParseOptions::default(), domain);
        registry.insert(address.clone(), &a);
        let reused = registry.get("/a.ts".into(), text, &address).expect("reuse");
        let node = a.source().root;
        let symbol = a.data.node_symbol.get(&node).copied().expect("file symbol");
        let first = ProgramSnapshot::new(vec![Arc::clone(&a), Arc::clone(&b)], 0).expect("first");
        let second = ProgramSnapshot::new(vec![b, reused], 0).expect("second");
        {
            let left = ProgramBinder::from_snapshot(&first);
            let right = ProgramBinder::from_snapshot(&second);
            assert_eq!(left.file_index_of_node(node), 0);
            assert_eq!(right.file_index_of_node(node), 1);
            assert!(std::ptr::eq(left.source(0), right.source(1)));
            assert!(std::ptr::eq(left.symbol(symbol), right.symbol(symbol)));
        }
        let (retained, expected_index) = if release_first {
            drop(first);
            (second, 1)
        } else {
            drop(second);
            (first, 0)
        };
        drop(a);
        assert_eq!(
            ProgramBinder::from_snapshot(&retained).file_index_of_node(node),
            expected_index
        );
        drop(retained);
        registry.purge();
        assert!(registry.is_empty());
    }
}

#[test]
fn failed_builder_publication_retains_no_document() {
    let domain = IdentityDomain::new();
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/foreign.ts",
        TextSnapshot::new("export {};", Default::default()),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("source identities");
    let mut builder = DocumentBuilder::new(source);
    let data = BinderWorker::bind_in_identity_domain(builder.source_mut(), &domain)
        .expect("bind identities")
        .into_bind_data();
    let mut foreign = EphemeralDocumentStore::new(IdentityDomain::new());
    assert_eq!(
        foreign.publish_builder(builder, data).unwrap_err(),
        EphemeralDocumentStoreError::SourceIdentityDomainMismatch,
    );
    assert!(foreign.documents().is_empty());
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
