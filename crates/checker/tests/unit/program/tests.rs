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
fn symbol_rooms_preserve_prototype_and_ambient_module_creation_order() {
    use tsc_binder::BindReservations;
    use tsc_types::{IdentitySpace, TypeOrderContext};

    let domain = IdentityDomain::new();
    let parse = |name: &str, text: &str| {
        tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
            name.to_owned(),
            TextSnapshot::new(text.to_owned(), Default::default()),
            ParseOptions::default(),
            None,
            &domain,
        )
        .unwrap()
    };
    let mut a = parse("/a.ts", "declare module \"alpha\" {} class A {}");
    let mut b = parse("/b.ts", "declare module \"beta\" {} class B {}");
    let ac = BindReservations::counts(&a);
    let bc = BindReservations::counts(&b);
    let mut room = domain
        .lease_rooms(&[vec![
            (IdentitySpace::Symbol, ac[0] + bc[0]),
            (IdentitySpace::PrivateNameSerial, ac[1] + bc[1]),
        ]])
        .unwrap()
        .pop()
        .unwrap();
    let br = BindReservations::finish_entry(&mut room, bc).unwrap();
    let ar = BindReservations::finish_entry(&mut room, ac).unwrap();
    room.seal().unwrap();
    let bind = |source: &mut SourceFile, reservation: BindReservations| {
        let mut worker = Binder::bind_reserved(
            source,
            reservation.symbols.range(),
            reservation.serials.range().start(),
        );
        assert!(worker
            .attach_reserved_leases(&domain, reservation.symbols, reservation.serials)
            .unwrap());
        worker.into_bind_data()
    };
    let bd = bind(&mut b, br);
    let ad = bind(&mut a, ar);
    let prototype = |data: &BindData| {
        let offset = data
            .symbols
            .symbols()
            .iter()
            .position(|symbol| symbol.escaped_name == "prototype")
            .unwrap();
        SymbolId::new(data.symbols.base() + u32::try_from(offset).unwrap())
    };
    let ap = prototype(&ad);
    let bp = prototype(&bd);
    assert!(ap.index() > bp.index(), "physical placement is reversed");
    let snapshot = ProgramSnapshot::new(
        vec![
            Arc::new(DocumentBuilder::new(a).freeze(ad)),
            Arc::new(DocumentBuilder::new(b).freeze(bd)),
        ],
        0,
    )
    .unwrap();
    let options = CompilerOptions::default();
    let state = CheckerState::from_snapshot(&snapshot, &options);
    let order = crate::type_order::OrderCtx::new(&state.binder, &state.links, &state.mappers, true);
    assert_eq!(
        order.compare_symbols(Some(ap), Some(bp)),
        std::cmp::Ordering::Less
    );
    let mut prototypes = [bp, ap];
    order.sort_symbols(&mut prototypes);
    assert_eq!(prototypes, [ap, bp]);
    let names: Vec<_> = state
        .get_ambient_modules()
        .into_iter()
        .map(|symbol| {
            state
                .binder
                .symbol(symbol)
                .escaped_name
                .as_js()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["\"alpha\"", "\"beta\""]);
}

#[test]
fn cached_documents_keep_numeric_ambient_order_when_membership_changes() {
    let domain = IdentityDomain::new();
    let a = bound_document_for_snapshot(
        "/a.ts",
        TextSnapshot::new("declare module \"alpha\" {}".to_owned(), Default::default()),
        &domain,
    );
    let b = bound_document_for_snapshot(
        "/b.ts",
        TextSnapshot::new("declare module \"beta\" {}".to_owned(), Default::default()),
        &domain,
    );
    let mut documents = [(a, "\"alpha\""), (b, "\"beta\"")];
    documents.sort_by_key(|(document, name)| {
        document.data.locals[&document.source().root]
            .get(*name)
            .copied()
            .unwrap()
    });
    let expected: Vec<_> = documents.iter().map(|(_, name)| *name).collect();
    let snapshot = ProgramSnapshot::new(
        documents
            .iter()
            .rev()
            .map(|(document, _)| Arc::clone(document))
            .collect(),
        0,
    )
    .unwrap();
    let options = CompilerOptions::default();
    let state = CheckerState::from_snapshot(&snapshot, &options);
    let names: Vec<_> = state
        .get_ambient_modules()
        .into_iter()
        .map(|symbol| {
            state
                .binder
                .symbol(symbol)
                .escaped_name
                .as_js()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, expected);
}

#[test]
fn overflowing_room_symbols_follow_the_normal_bind_reservations() {
    use tsc_binder::BindReservations;
    use tsc_types::IdentitySpace;

    let domain = IdentityDomain::new();
    let text = format!(
        "{}function f(x) {{ return x; }}\nclass Overflow {{}}",
        "/** @overload\n * @param {string} x\n * @returns {string}\n */\n".repeat(32)
    );
    let mut overflow = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/overflow.js",
        TextSnapshot::new(text, Default::default()),
        ParseOptions {
            script_kind: tsc_syntax::ScriptKind::Js,
            ..Default::default()
        },
        None,
        &domain,
    )
    .unwrap();
    let mut normal = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/normal.ts",
        TextSnapshot::new("class Normal {}".to_owned(), Default::default()),
        ParseOptions::default(),
        None,
        &domain,
    )
    .unwrap();
    let oc = BindReservations::counts(&overflow);
    let nc = BindReservations::counts(&normal);
    let mut room = domain
        .lease_rooms(&[vec![
            (IdentitySpace::Symbol, oc[0] + nc[0]),
            (IdentitySpace::PrivateNameSerial, oc[1] + nc[1]),
        ]])
        .unwrap()
        .pop()
        .unwrap();
    let or = BindReservations::finish_entry(&mut room, oc).unwrap();
    let nr = BindReservations::finish_entry(&mut room, nc).unwrap();
    room.seal().unwrap();
    let mut normal_worker =
        Binder::bind_reserved(&mut normal, nr.symbols.range(), nr.serials.range().start());
    assert!(normal_worker
        .attach_reserved_leases(&domain, nr.symbols, nr.serials)
        .unwrap());
    let normal_data = normal_worker.into_bind_data();
    let mut worker = Binder::bind_reserved(
        &mut overflow,
        or.symbols.range(),
        or.serials.range().start(),
    );
    assert!(
        worker.symbols.len() > oc[0] as usize,
        "JS overloads exceed the parser's bound"
    );
    assert!(!worker
        .attach_reserved_leases(&domain, or.symbols.clone(), or.serials.clone())
        .unwrap());
    worker.relocate_into_identity_domain(&domain).unwrap();
    let overflow_data = worker.into_bind_data();
    let prototype = |data: &BindData| {
        let offset = data
            .symbols
            .symbols()
            .iter()
            .position(|symbol| symbol.escaped_name == "prototype")
            .unwrap();
        SymbolId::new(data.symbols.base() + u32::try_from(offset).unwrap())
    };
    let op = prototype(&overflow_data);
    let np = prototype(&normal_data);
    let reference = IdentityDomain::with_limits(Default::default()).unwrap();
    let reserved = reference.lease(IdentitySpace::Symbol, oc[0]).unwrap();
    let relocated = reference
        .lease(
            IdentitySpace::Symbol,
            u32::try_from(overflow_data.symbols.len()).unwrap(),
        )
        .unwrap();
    let hole = reserved.range().start();
    drop(reserved);
    let reused = reference.lease(IdentitySpace::Symbol, nc[0]).unwrap();
    assert_eq!(reused.range().start(), hole);
    assert!(reused.range().start() < relocated.range().start());
    let documents = vec![
        Arc::new(DocumentBuilder::new(overflow).freeze(overflow_data)),
        Arc::new(DocumentBuilder::new(normal).freeze(normal_data)),
    ];
    for lib_count in [0, 1] {
        let snapshot = ProgramSnapshot::new(documents.clone(), lib_count).unwrap();
        let program = ProgramBinder::from_snapshot(&snapshot);
        assert_eq!(
            program.compare_symbol_creation_order(np, op),
            std::cmp::Ordering::Less,
            "normal symbols precede an overflowing bind, including library-hole reuse"
        );
    }
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
            let (mut scopes, mut empty) = (0, 0);
            for node in source.arena.node_ids() {
                let found = state.binder.locals_of(node);
                assert_eq!(
                    found.is_some(),
                    tsc_binder::published_has_locals(node),
                    "{node:?}"
                );
                match (found, bound.locals.get(&node)) {
                    (Some(found), Some(kept)) => assert!(std::ptr::eq(found, kept), "{node:?}"),
                    // The bound file keeps only the tables with entries.
                    (Some(found), None) => {
                        assert!(found.is_empty(), "{node:?}");
                        empty += 1;
                    }
                    (None, kept) => assert!(kept.is_none(), "{node:?}"),
                }
                scopes += usize::from(found.is_some());
            }
            assert!(scopes >= 5, "the source file, functions, blocks and loop");
            assert!(empty >= 1, "the method without parameters or locals");
        },
    );
}

#[test]
fn prepared_bind_results_keep_source_order_around_missing_and_overflowing_binds() {
    use std::num::NonZeroUsize;
    use tsc_binder::BindReservations;
    use tsc_types::IdentitySpace;
    for worker_count in [1, 4] {
        let domain = IdentityDomain::new();
        let mut sources = [
            ("/first.ts", "const first = 1;"),
            ("/fresh.ts", "function fresh() { return 2; }"),
            (
                "/overflow.ts",
                "export class C { #value = 3; read() { return this.#value; } }",
            ),
            ("/last.ts", "const last = 4;"),
        ]
        .into_iter()
        .map(|(name, text)| {
            DocumentBuilder::new(
                tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                    name.to_owned(),
                    TextSnapshot::new(text.to_owned(), Default::default()),
                    ParseOptions::default(),
                    None,
                    &domain,
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
        for (index, source) in sources.iter_mut().enumerate() {
            if index == 1 {
                continue;
            }
            let reservation = if index == 2 {
                BindReservations {
                    symbols: domain.lease(IdentitySpace::Symbol, 1).unwrap(),
                    serials: domain.lease(IdentitySpace::PrivateNameSerial, 1).unwrap(),
                }
            } else {
                BindReservations::lease(source.source(), &domain).unwrap()
            };
            let mut binder = Binder::bind_reserved(
                source.source_mut(),
                reservation.symbols.range(),
                reservation.serials.range().start(),
            );
            assert_eq!(
                binder
                    .attach_reserved_leases(
                        &domain,
                        reservation.symbols.clone(),
                        reservation.serials.clone()
                    )
                    .unwrap(),
                index != 2
            );
            source.bind_data = Some(Box::new(binder.into_bind_data()));
            source.bind_reservations = Some(reservation);
        }
        let inputs = sources
            .iter_mut()
            .map(DocumentBuilder::bind_input)
            .collect();
        let results = crate::bind_reserved_in_program_order(
            inputs,
            &domain,
            crate::WorkerBudget::new(NonZeroUsize::new(worker_count).unwrap()),
            None,
        );
        assert_eq!(results.len(), sources.len());
        for (source, data) in sources.iter_mut().zip(results) {
            assert!(data.identity_owned_by(&domain));
            let expected = data.clone();
            drop(data);
            let mut direct = Binder::with_bases(
                source.source_mut(),
                expected.private_name_serial_base,
                expected.symbols.base(),
            );
            direct.bind_source_file();
            let direct = direct.into_bind_data();
            assert_eq!(expected.symbols, direct.symbols);
            assert_eq!(expected.node_symbol, direct.node_symbol);
            assert_eq!(expected.node_local_symbol, direct.node_local_symbol);
            assert_eq!(expected.locals, direct.locals);
            assert_eq!(expected.assigned_symbol_ids, direct.assigned_symbol_ids);
            assert_eq!(expected.classifiable_names, direct.classifiable_names);
            assert_eq!(expected.bind_diagnostics, direct.bind_diagnostics);
        }
    }
}

#[test]
fn snapshot_owner_routes_are_shared_across_concurrent_sessions_and_clones() {
    let domain = IdentityDomain::new();
    let a = bound_document_for_snapshot(
        "/a.ts",
        TextSnapshot::new("export const a = 1;".to_owned(), Default::default()),
        &domain,
    );
    let b = bound_document_for_snapshot(
        "/b.ts",
        TextSnapshot::new("export const b = 2;".to_owned(), Default::default()),
        &domain,
    );
    let a_node = a.source().root;
    let b_node = b.source().root;
    let first = ProgramSnapshot::new(vec![Arc::clone(&a), Arc::clone(&b)], 0).unwrap();
    let cloned = first.clone();
    assert!(first.owner_routes.get().is_none());
    std::thread::scope(|scope| {
        let left = scope.spawn(|| ProgramBinder::from_snapshot(&first));
        let right = ProgramBinder::from_snapshot(&cloned);
        let left = left.join().unwrap();
        assert!(Arc::ptr_eq(&left.node_owners, &right.node_owners));
        assert!(Arc::ptr_eq(&left.symbol_owners, &right.symbol_owners));
        assert!(Arc::ptr_eq(&left.node_files.pages, &right.node_files.pages));
        assert!(Arc::ptr_eq(
            &left.symbol_files.pages,
            &right.symbol_files.pages
        ));
        assert_eq!(left.file_index_of_node(a_node), 0);
        assert_eq!(right.file_index_of_node(b_node), 1);
    });
    let reordered = ProgramSnapshot::new(vec![b, a], 0).unwrap();
    let left = ProgramBinder::from_snapshot(&first);
    let right = ProgramBinder::from_snapshot(&reordered);
    assert!(!Arc::ptr_eq(&left.node_owners, &right.node_owners));
    assert_eq!(left.file_index_of_node(a_node), 0);
    assert_eq!(right.file_index_of_node(a_node), 1);
    drop(left);
    drop(first);
    assert_eq!(
        ProgramBinder::from_snapshot(&cloned).file_index_of_node(b_node),
        1
    );
}

#[test]
fn snapshot_checker_tables_are_made_once_for_every_view() {
    let domain = IdentityDomain::new();
    let document = |path: &str, text: &str| {
        bound_document_for_snapshot(
            path,
            TextSnapshot::new(text.to_owned(), Default::default()),
            &domain,
        )
    };
    let documents = vec![
        document("/src/a.ts", "export const a = 1;"),
        document("/src/./b.ts", "export const b = 2;"),
        // A later file shadows an earlier one of the same name.
        document("/src/a.ts", "export const a = 3;"),
    ];
    let snapshot = ProgramSnapshot::new(documents.clone(), 0).unwrap();
    let cloned = snapshot.clone();
    // A view does not build the tables; the first reader does, once, for
    // every view of the snapshot and of its clones.
    let left = ProgramBinder::from_snapshot(&snapshot);
    let right = ProgramBinder::from_snapshot(&cloned);
    assert!(snapshot.checker_tables.get().is_none());
    let paths = left.path_index();
    assert!(std::ptr::eq(paths, right.path_index()));
    assert!(std::ptr::eq(
        paths,
        &cloned.checker_tables.get().unwrap().path_index
    ));
    assert_eq!(paths.len(), 2);
    assert_eq!(paths.get("/src/a.ts".as_bytes()), Some(&2));
    assert_eq!(paths.get("/src/b.ts".as_bytes()), Some(&1));
    // Made ahead of the checkers: the table is there before any view asks,
    // and a view reads that one.
    let warmed = ProgramSnapshot::new(documents, 0).unwrap();
    assert!(warmed.checker_tables.get().is_none());
    warmed.warm_path_index();
    assert!(warmed.has_path_index());
    let made = warmed.checker_tables.get().expect("warmed tables");
    assert!(std::ptr::eq(
        ProgramBinder::from_snapshot(&warmed).path_index(),
        &made.path_index
    ));
    assert_eq!(made.path_index, *paths);
}

#[test]
fn snapshot_checker_starts_name_the_files_the_globals_merge_reads() {
    let domain = IdentityDomain::new();
    let document = |path: &str, text: &str| {
        bound_document_for_snapshot(
            path,
            TextSnapshot::new(text.to_owned(), Default::default()),
            &domain,
        )
    };
    let documents = vec![
        // 0: an external module with nothing for the globals.
        document("/src/module.ts", "export const a = 1;"),
        // 1: a global script.
        document("/src/script.ts", "var g = 1;"),
        // 2: an external module whose symbol has global exports.
        document(
            "/types/umd.d.ts",
            "export as namespace Umd;\nexport const u: number;",
        ),
        // 3: another module with nothing.
        document("/src/other.ts", "import { a } from './module';\na;"),
        // 4: a file without a module indicator is a global script.
        document("/src/empty.ts", ""),
        // 5: a global script outside the Program.
        document("/outside/script.ts", "var h = 2;"),
    ];
    let mut facts = vec![ProgramFileFacts::ORDINARY; documents.len()];
    facts[5] = ProgramFileFacts::OUTSIDE_PROGRAM;
    let snapshot = ProgramSnapshot::new_with_file_facts(documents.clone(), facts.clone()).unwrap();
    let view = ProgramBinder::from_snapshot(&snapshot);
    assert!(snapshot.checker_starts.get().is_none());
    let starts = view.checker_starts();
    assert_eq!(starts.global_contributors, [1, 2, 4]);
    let files = 0..documents.len();
    assert_eq!(
        starts.nodes,
        files
            .clone()
            .map(|index| view.source(index).arena.len())
            .sum::<usize>()
    );
    assert_eq!(
        starts.symbols,
        files
            .clone()
            .map(|index| view.file(index).symbols.len())
            .sum::<usize>()
    );
    assert_eq!(
        starts.node_id_end,
        files
            .clone()
            .map(|index| view.source(index).arena.node_end() as usize)
            .max()
            .unwrap()
    );
    assert_eq!(
        starts.symbol_id_end,
        files
            .map(|index| view.file(index).symbols.next_id().index() as usize)
            .max()
            .unwrap()
    );
    // One table for every view of the snapshot and of its clones.
    assert!(std::ptr::eq(
        starts,
        ProgramBinder::from_snapshot(&snapshot.clone()).checker_starts()
    ));
    // Made ahead of the checkers: the routes and the table are there before
    // any view asks.
    let warmed = ProgramSnapshot::new_with_file_facts(documents, facts).unwrap();
    assert!(warmed.owner_routes.get().is_none() && warmed.checker_starts.get().is_none());
    assert!(!warmed.has_checker_starts() && !warmed.has_path_index());
    warmed.warm_checker_starts();
    assert!(warmed.owner_routes.get().is_some());
    assert!(warmed.has_checker_starts() && !warmed.has_path_index());
    assert_eq!(warmed.checker_starts.get(), Some(starts));
}

#[test]
fn snapshot_owner_route_failures_remain_local_to_the_snapshot() {
    let first_domain = IdentityDomain::new();
    let second_domain = IdentityDomain::new();
    let a = bound_document_for_snapshot(
        "/a.ts",
        TextSnapshot::new("export const a = 1;".to_owned(), Default::default()),
        &first_domain,
    );
    let b = bound_document_for_snapshot(
        "/b.ts",
        TextSnapshot::new("export const b = 2;".to_owned(), Default::default()),
        &second_domain,
    );
    let invalid = ProgramSnapshot::new(vec![Arc::clone(&a), b], 0).unwrap();
    for snapshot in [&invalid, &invalid.clone()] {
        assert!(matches!(
            ProgramBinder::try_from_snapshot(snapshot),
            Err(ProgramIdentityError::IdentityDomainMismatch { file: 1 })
        ));
    }
    let valid = ProgramSnapshot::new(vec![a], 0).unwrap();
    assert!(ProgramBinder::try_from_snapshot(&valid).is_ok());
}

#[test]
fn authoritative_sources_follow_the_snapshot_and_install_atomically() {
    use crate::modules::ModuleResolutionMode;
    use crate::{
        AuthoritativeModuleFailure, AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider,
        AuthoritativeModuleRequest, AuthoritativeModuleResolution, AuthoritativeResolutionMode,
        AuthoritativeSourceMetadata, AuthoritativeSourceToken, AuthoritativeSources,
    };

    struct Provider;
    impl AuthoritativeModuleProvider for Provider {
        fn resolve_module(
            &self,
            _: AuthoritativeModuleRequest<'_>,
        ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
            unreachable!("the fixture has no imports")
        }
    }
    let domain = IdentityDomain::new();
    let doc = |name| {
        bound_document_for_snapshot(name, TextSnapshot::new("", Default::default()), &domain)
    };
    let a = doc("/a.ts");
    let b = doc("/b.ts");
    let snapshot = ProgramSnapshot::new(vec![a.clone(), b.clone()], 0).unwrap();
    let reference = tsc_program::ProjectReferenceFile {
        options: Arc::new(tsc_program::ReferencedProjectOptions {
            emit_module_kind: 99,
            module_resolution_kind: 3,
            resolve_package_json_exports: true,
            resolve_package_json_imports: false,
            preserves_const_enums: false,
            common_source_directory: "/src".into(),
            out_dir: Some("/out".into()),
        }),
        output_declaration: true,
        source_from_project_reference: false,
    };
    let row = |name: &str, token| AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(token),
        file_name: name.into(),
        may_be_emitted: token == 7,
        implied_node_format: Some(AuthoritativeResolutionMode::CommonJs),
        implied_node_format_for_emit: Some(AuthoritativeResolutionMode::EsNext),
        project_reference: Some(reference.clone()),
    };
    let rows = vec![row("/a.ts", 7), row("/b.ts", 3)];
    let valid = AuthoritativeSources::new(&snapshot, rows.clone());
    let mut replacement_rows = rows.clone();
    replacement_rows[0].implied_node_format = None;
    replacement_rows[0].project_reference = None;
    let replacement = AuthoritativeSources::new(&snapshot, replacement_rows);
    // Name validation wins over a duplicate on the same row. A failed
    // install keeps the previous projection and the checker's populated memo.
    let invalid_rows = [
        (
            vec![row("/wrong.ts", 7)],
            "authoritative source metadata has 1 rows for 2 checker files",
        ),
        (
            vec![row("/a.ts", 7), row("/wrong.ts", 7)],
            "authoritative source metadata row 1 names \"/wrong.ts\", checker file is \"/b.ts\"",
        ),
        (
            vec![row("/a.ts", 7), row("/b.ts", 7)],
            "authoritative source token 7 is shared by checker files 0 and 1",
        ),
        (
            vec![row("/a.ts", 7), row("/b.ts", 7), row("/extra.ts", 7)],
            "authoritative source metadata has 3 rows for 2 checker files",
        ),
    ];
    let invalid: Vec<_> = invalid_rows
        .into_iter()
        .map(|(rows, error)| (AuthoritativeSources::new(&snapshot, rows), error))
        .collect();
    // Association sees the last duplicate within the snapshot, even when
    // the invalid input has an extra copy beyond the last document.
    assert_eq!(
        invalid[3]
            .0
            .file_index_in_snapshot(AuthoritativeSourceToken(7), 2),
        Some(1)
    );
    let options = CompilerOptions::default();
    let mut checker = CheckerState::from_snapshot(&snapshot, &options);
    checker
        .install_authoritative_module_provider(&Provider, &valid)
        .unwrap();
    assert_eq!(
        checker.implied_node_format_for_file(a.source().root),
        Some(ModuleResolutionMode::CommonJs)
    );
    assert_eq!(
        checker.implied_node_format_for_emit(a.source().root),
        Some(ModuleResolutionMode::EsNext)
    );
    assert_eq!(checker.project_reference_of_file(0), Some(&reference));
    for (sources, error) in &invalid {
        assert_eq!(
            checker.install_authoritative_module_provider(&Provider, sources),
            Err(AuthoritativeModuleFailure::InvalidMetadata {
                detail: (*error).into()
            })
        );
        assert!(std::ptr::eq(checker.authoritative_sources, &valid));
        assert!(!checker.implied_node_format_memo.borrow().is_empty());
    }
    let mut peer = CheckerState::from_snapshot(&snapshot, &options);
    peer.install_authoritative_module_provider(&Provider, &valid)
        .unwrap();
    assert!(std::ptr::eq(
        peer.authoritative_sources,
        checker.authoritative_sources
    ));
    assert!(peer.implied_node_format_memo.borrow().is_empty());
    let first = AuthoritativeModuleFailure::InvalidMetadata {
        detail: "first lookup".into(),
    };
    checker.record_authoritative_module_failure(first.clone());
    checker.record_authoritative_module_failure(AuthoritativeModuleFailure::InvalidMetadata {
        detail: "second lookup".into(),
    });
    assert!(peer.take_authoritative_module_failure().is_none());
    checker
        .install_authoritative_module_provider(&Provider, &replacement)
        .unwrap();
    assert_eq!(checker.take_authoritative_module_failure(), Some(first));
    assert!(checker.implied_node_format_memo.borrow().is_empty());
    assert_eq!(checker.implied_node_format_for_file(a.source().root), None);
    assert!(checker.project_reference_of_file(0).is_none());

    let reversed = ProgramSnapshot::new(vec![b, a], 0).unwrap();
    let reordered = AuthoritativeSources::new(&reversed, rows.into_iter().rev().collect());
    let mut next = CheckerState::from_snapshot(&reversed, &options);
    next.install_authoritative_module_provider(&Provider, &reordered)
        .unwrap();
    assert!(std::ptr::eq(next.authoritative_sources, &reordered));
    assert_eq!(valid.file_index(AuthoritativeSourceToken(7)), Some(0));
    assert_eq!(reordered.file_index(AuthoritativeSourceToken(7)), Some(1));
}

/// The program is asked for a module once at an import declaration: the
/// declaration's own check and every alias it introduces start from the same
/// node, and the resolved answer is kept by that node and the specifier. A
/// second provider's installation forgets the first one's answers.
#[test]
fn a_resolved_module_is_asked_of_the_provider_once_an_import_declaration() {
    use crate::{
        AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
        AuthoritativeModuleResolution, AuthoritativeResolvedModule, AuthoritativeSourceMetadata,
        AuthoritativeSourceToken, AuthoritativeSources,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Provider {
        requests: AtomicUsize,
    }
    impl AuthoritativeModuleProvider for Provider {
        fn resolve_module(
            &self,
            request: AuthoritativeModuleRequest<'_>,
        ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
            self.requests.fetch_add(1, Ordering::Relaxed);
            assert_eq!(request.source_token, AuthoritativeSourceToken(1));
            assert_eq!(request.specifier, "./b");
            Ok(AuthoritativeModuleResolution::Resolved(
                AuthoritativeResolvedModule {
                    target_token: AuthoritativeSourceToken(2),
                    resolved_file_name: "/b.ts".into(),
                    resolved_using_ts_extension: false,
                    is_custom_resolution: false,
                    is_tsx: false,
                    is_arbitrary_extension: false,
                    is_external_library_import: false,
                    package_id: None,
                    alternate_result: None,
                    types_package_exists: false,
                    package_bundles_types: false,
                },
            ))
        }
    }

    let domain = IdentityDomain::new();
    let doc = |name, text: &str| {
        bound_document_for_snapshot(
            name,
            TextSnapshot::new(text.to_owned(), Default::default()),
            &domain,
        )
    };
    let documents = vec![
        doc(
            "/a.ts",
            "import { x, y, z } from \"./b\";\n\
             export const p = x + y;\nexport const q: typeof z = z;\nexport const r = [x, y, z];\n",
        ),
        doc(
            "/b.ts",
            "export const x = 1;\nexport const y = 2;\nexport const z = 3;\n",
        ),
    ];
    let snapshot = ProgramSnapshot::new(documents, 0).unwrap();
    let row = |name: &str, token| AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(token),
        file_name: name.into(),
        may_be_emitted: true,
        implied_node_format: None,
        implied_node_format_for_emit: None,
        project_reference: None,
    };
    let sources = AuthoritativeSources::new(&snapshot, vec![row("/a.ts", 1), row("/b.ts", 2)]);
    let options = CompilerOptions::default();
    let provider = Provider {
        requests: AtomicUsize::new(0),
    };
    let mut checker = CheckerState::from_snapshot(&snapshot, &options);
    checker
        .install_authoritative_module_provider(&provider, &sources)
        .unwrap();
    for file in 0..snapshot.documents().len() {
        checker.check_source_file(file);
    }
    assert!(checker.take_authoritative_module_failure().is_none());
    let module_errors: Vec<_> = checker
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 2305 | 2307 | 2792))
        .collect();
    assert!(module_errors.is_empty(), "{module_errors:?}");
    // Three aliases, each resolved along more than one path, and the
    // declaration's own check: one request without the kept answer would
    // be ten or more.
    let asked = provider.requests.load(Ordering::Relaxed);
    assert!((1..=2).contains(&asked), "{asked} requests");

    // Another provider: nothing of the first one's is answered from the
    // cache.
    let second = Provider {
        requests: AtomicUsize::new(0),
    };
    let mut fresh = CheckerState::from_snapshot(&snapshot, &options);
    fresh
        .install_authoritative_module_provider(&provider, &sources)
        .unwrap();
    fresh.check_source_file(0);
    let before = provider.requests.load(Ordering::Relaxed);
    assert!(before > asked);
    fresh
        .install_authoritative_module_provider(&second, &sources)
        .unwrap();
    assert!(fresh.resolved_module_cache.borrow().is_empty());
    assert_eq!(second.requests.load(Ordering::Relaxed), 0);
}

/// A checker of a pool reserves its lookup tables (the type-list map, the
/// literal map, the instantiation map and its front, the relation pairs)
/// for half the program; the one checker of a program for all of it. The
/// reservation is capacity alone: both checkers check the program to the
/// same diagnostics and the same types in the same order.
#[test]
fn a_pooled_checker_reserves_its_lookup_tables_for_half_the_program() {
    assert_eq!(CheckerState::lookup_share(0), 1);
    assert_eq!(CheckerState::lookup_share(1), 1);
    assert_eq!(CheckerState::lookup_share(2), 2);
    assert_eq!(CheckerState::lookup_share(12), 2);

    let domain = IdentityDomain::new();
    // Forty thousand declarations: reservations well above the tables'
    // smallest sizes, so that half of one is a smaller table.
    let mut declarations = String::from(
        "interface Object {} interface Function {} interface CallableFunction {} \
         interface NewableFunction {} interface IArguments {} interface String {} \
         interface Number {} interface Boolean {} interface RegExp {} \
         interface Array<T> { length: number }\n",
    );
    for index in 0..40_000 {
        declarations.push_str(&format!("declare const v{index}: number;\n"));
    }
    let documents = vec![
        bound_document_for_snapshot(
            "/lib.d.ts",
            TextSnapshot::new(declarations, Default::default()),
            &domain,
        ),
        bound_document_for_snapshot(
            "/use.ts",
            TextSnapshot::new(
                "const a: number[] = [v1, v2]; const b: string = v3;".to_owned(),
                Default::default(),
            ),
            &domain,
        ),
    ];
    let snapshot = ProgramSnapshot::new(documents, 1).unwrap();
    let options = CompilerOptions::default();
    let run = |pool_size| {
        let mut state =
            CheckerState::from_snapshot_deferring_globals(&snapshot, &options, false, pool_size);
        let reserved = (
            state.tables.reserved_lookup_bytes(),
            state.relations.memory_usage().1,
        );
        state.initialize_deferred_program_globals();
        state.merge_module_augmentations_around(|checker| {
            checker.materialize_init_global_diagnostics()
        });
        for file in 0..snapshot.documents().len() {
            state.check_source_file(file);
        }
        let creation_order: Vec<_> = (0..state.tables.len())
            .map(|index| {
                state
                    .tables
                    .type_of(tsc_types::TypeId::new(index as u32))
                    .clone()
            })
            .collect();
        (reserved, state.diagnostics.to_vec(), creation_order)
    };
    let (whole, whole_diagnostics, whole_types) = run(1);
    let (pooled, pooled_diagnostics, pooled_types) = run(4);
    assert!(
        pooled.0 < whole.0,
        "type tables: {} against {}",
        pooled.0,
        whole.0
    );
    assert!(
        pooled.1 < whole.1,
        "relation pairs: {} against {}",
        pooled.1,
        whole.1
    );
    // `const b: string = v3` is an error in both.
    assert!(!whole_diagnostics.is_empty());
    assert_eq!(pooled_diagnostics, whole_diagnostics);
    assert_eq!(pooled_types, whole_types);
}

#[test]
fn snapshot_augmentations_keep_both_merge_passes_in_checker_order() {
    let domain = IdentityDomain::new();
    let sources = [
        ("/lib.d.ts", "interface Object {} interface Function {} interface CallableFunction {} interface NewableFunction {} interface IArguments {} interface String {} interface Number {} interface Boolean {} interface RegExp {} interface Array<T> { length: number }"),
        ("/ambient.d.ts", "declare module 'service' { export interface Item { value: string } }"),
        ("/first.ts", "export {}; declare global { interface Array<T> { first: T } } declare module 'service' { interface Item { first: number } }"),
        ("/second.ts", "export {}; declare global { interface Array<T> { second: T } } declare module 'service' { interface Item { second: boolean } }"),
        ("/use.ts", "import type {Item} from 'service'; declare const v: Item; declare const a: number[]; const n: number = a.first + a.second + v.first; const b: boolean = v.second; const wrong: number = v.value;"),
    ];
    let documents = sources
        .into_iter()
        .map(|(name, text)| {
            bound_document_for_snapshot(name, TextSnapshot::new(text, Default::default()), &domain)
        })
        .collect();
    let snapshot = ProgramSnapshot::new(documents, 1).unwrap();
    let inputs = crate::merge::AugmentationInputs::for_snapshot(&snapshot);
    let options = CompilerOptions::default();
    let run = |shared| {
        let mut state =
            CheckerState::from_snapshot_deferring_globals(&snapshot, &options, false, 1);
        state.initialize_deferred_program_globals();
        let between =
            |checker: &mut CheckerState<'_>| checker.materialize_init_global_diagnostics();
        if shared {
            state.merge_augmentation_inputs(&inputs, between);
        } else {
            state.merge_module_augmentations_around(between);
        }
        for file in 0..snapshot.documents().len() {
            state.check_source_file(file);
        }
        let creation_order: Vec<_> = (0..state.tables.len())
            .map(|index| {
                state
                    .tables
                    .type_of(tsc_types::TypeId::new(index as u32))
                    .clone()
            })
            .collect();
        let counts = crate::CheckerCounts::of(&state, true);
        (
            state.diagnostics.to_vec(),
            creation_order,
            (counts.types, counts.instantiations, counts.symbols),
        )
    };
    let standalone = run(false);
    assert_eq!(standalone.2, (79, 2, 58));
    assert_eq!(
        standalone
            .0
            .iter()
            .map(|row| row.code())
            .collect::<Vec<_>>(),
        [2322, 6133, 6133, 6133]
    );
    assert!(standalone.0.iter().any(|row| {
        row.code() == 2322 && row.file_name.as_ref().is_some_and(|name| name == "/use.ts")
    }));
    assert_eq!(run(true), standalone);
}

/// A walk up one node's ancestors asks for the locals of each scope. Over
/// program-owned documents the walk routes to its file's tables once and
/// reads the same tables `locals_of` reads, scope by scope, in either file
/// of a program; a new walk routes again.
#[test]
fn a_scope_walk_routes_to_its_files_locals_once_and_reads_the_same_tables() {
    let domain = IdentityDomain::new();
    let document = |path: &str, text: &str| {
        bound_document_for_snapshot(
            path,
            TextSnapshot::new(text.to_owned(), DocumentVersion::default()),
            &domain,
        )
    };
    let documents = vec![
        document(
            "/a.ts",
            "export function outerA(p: number) { const a = p; function innerA() { const deepA = a; return deepA; } return innerA; }",
        ),
        document(
            "/b.ts",
            "export namespace N { export function outerB(q: string) { { let block = q; class C { m() { const deepB = block; return deepB; } } return C; } } }",
        ),
    ];
    let snapshot = ProgramSnapshot::new(documents, 0).unwrap();
    let program = ProgramBinder::from_snapshot(&snapshot);
    for (file, deep) in [(0, "deepA"), (1, "deepB")] {
        let start = program
            .source(file)
            .arena
            .node_ids()
            .filter(|&node| {
                let record = tsc_syntax::published_node(node);
                record.kind == tsc_syntax::SyntaxKind::Identifier
                    && record
                        .as_identifier()
                        .is_some_and(|data| data.escaped_text.identifier_text() == deep)
            })
            .max_by_key(|&node| tsc_syntax::published_node(node).pos)
            .expect("the deepest name");
        let mut walk = WalkLocals::default();
        let mut scopes_with_locals = 0;
        let mut current = Some(start);
        while let Some(node) = current {
            let in_walk = program.locals_of_in_walk(node, &mut walk);
            let routed = program.locals_of(node);
            match (in_walk, routed) {
                (Some(kept), Some(table)) => {
                    assert!(std::ptr::eq(kept, table));
                    scopes_with_locals += 1;
                }
                (None, None) => {}
                _ => panic!("the walk and the route disagree at {node:?}"),
            }
            current = tsc_syntax::published_node(node).parent;
        }
        // The method or function, the enclosing function and the file (and
        // on the way the block, the class and the namespace of the second).
        assert!(scopes_with_locals >= 3, "{deep}: {scopes_with_locals}");
        assert!(
            matches!(walk.0, Some(Some(_))),
            "the walk kept its file's tables"
        );
    }
}
