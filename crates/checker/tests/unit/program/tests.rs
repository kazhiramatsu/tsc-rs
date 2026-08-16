use super::*;
use crate::state::CheckerState;
use std::sync::Arc;
use tsc_binder::BinderWorker;
use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan, DocumentVersion, TextSnapshot};
use tsc_syntax::{parse_source_file, IncrementalParseOptions, ParseOptions};
use tsc_types::{CompilerOptions, IdentityDomain, IdentitySpace};

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
fn registry_reuses_unchanged_parse_and_bind_and_releases_versions() {
    let domain = IdentityDomain::reclaiming();
    let path = "/registry.ts";
    let snapshot_v1 = TextSnapshot::new("export const value = 1;", DocumentVersion::new("1"));
    let snapshot_v2 = TextSnapshot::new("export const value = 2;", DocumentVersion::new("2"));
    let address = DocumentAddress::new(
        "test-registry",
        path,
        DocumentScriptKind::TypeScript,
        CompilerOptions::default(),
    );
    let mut registry = DocumentRegistry::new("test-registry");
    let mut parses = 0u32;
    let mut binds = 0u32;

    let first = registry
        .acquire(address.clone(), Arc::clone(&snapshot_v1), || {
            parses += 1;
            binds += 1;
            bound_document_for_snapshot(path, Arc::clone(&snapshot_v1), &domain)
        })
        .expect("first version publishes");
    let second = registry
        .acquire(address.clone(), Arc::clone(&snapshot_v1), || {
            parses += 1;
            binds += 1;
            bound_document_for_snapshot(path, Arc::clone(&snapshot_v1), &domain)
        })
        .expect("same version reuses");
    assert_eq!(parses, 1);
    assert_eq!(binds, 1);
    assert!(Arc::ptr_eq(first.document(), second.document()));
    assert_eq!(registry.active_entry_count(), 1);
    assert_eq!(registry.active_reference_count(), 2);

    let first_program =
        ProgramSnapshot::new(vec![first.document().clone()], 0).expect("first snapshot");
    let second_program =
        ProgramSnapshot::new(vec![second.document().clone()], 0).expect("second snapshot");
    assert!(Arc::ptr_eq(
        first_program.document(0),
        second_program.document(0)
    ));

    let newer = registry
        .update(address.clone(), Arc::clone(&snapshot_v2), || {
            parses += 1;
            binds += 1;
            bound_document_for_snapshot(path, Arc::clone(&snapshot_v2), &domain)
        })
        .expect("new version publishes beside the live old version");
    assert_eq!(parses, 2);
    assert_eq!(binds, 2);
    assert_eq!(registry.active_entry_count(), 2);
    assert_eq!(registry.active_reference_count(), 3);

    registry.release(first).expect("release first snapshot");
    assert_eq!(registry.active_reference_count(), 2);
    registry.release(second).expect("release second snapshot");
    assert_eq!(registry.active_entry_count(), 1);
    registry.release(newer).expect("release updated snapshot");
    assert_eq!(registry.active_entry_count(), 0);
    assert_eq!(registry.active_reference_count(), 0);
}

#[test]
fn registry_rejects_same_version_text_replacement() {
    let domain = IdentityDomain::reclaiming();
    let path = "/registry-version.ts";
    let snapshot = TextSnapshot::new("export const value = 1;", DocumentVersion::new("1"));
    let replacement = TextSnapshot::new("export const value = 2;", DocumentVersion::new("1"));
    let document = bound_document_for_snapshot(path, Arc::clone(&snapshot), &domain);
    let address = DocumentAddress::new(
        "test-registry",
        path,
        DocumentScriptKind::TypeScript,
        CompilerOptions::default(),
    );
    let mut registry = DocumentRegistry::new("test-registry");
    let lease = registry
        .acquire(address.clone(), Arc::clone(&snapshot), || {
            Arc::clone(&document)
        })
        .expect("initial document");
    let error = registry
        .acquire(address, replacement, || Arc::clone(&document))
        .expect_err("equal host versions cannot fork text");
    assert!(matches!(
        error,
        DocumentRegistryError::VersionTextMismatch { .. }
    ));
    registry.release(lease).expect("release initial document");
}

#[test]
fn registry_incrementally_reparses_only_the_changed_document_and_reclaims_versions() {
    let domain = IdentityDomain::reclaiming();
    let changed_path = "/changed.ts";
    let stable_path = "/stable.ts";
    let changed_v1_text = concat!(
        "export const before = 1;\n",
        "export function value() { return 1; }\n",
        "export const after = 3;\n",
    );
    let edit_start = changed_v1_text.rfind("return 1").unwrap() + "return ".len();
    let mut changed_v2_text = changed_v1_text.to_owned();
    changed_v2_text.replace_range(edit_start..edit_start + 1, "2");
    let changed_v1 = TextSnapshot::new(changed_v1_text, DocumentVersion::new("changed-1"));
    let changed_v2 = TextSnapshot::new(changed_v2_text, DocumentVersion::new("changed-2"));
    let stable = TextSnapshot::new(
        "export interface Stable { value: string }",
        DocumentVersion::new("stable-1"),
    );
    let options = CompilerOptions::default();
    let changed_address = DocumentAddress::new(
        "incremental-registry",
        changed_path,
        DocumentScriptKind::TypeScript,
        options.clone(),
    );
    let stable_address = DocumentAddress::new(
        "incremental-registry",
        stable_path,
        DocumentScriptKind::TypeScript,
        options,
    );
    let mut registry = DocumentRegistry::new("incremental-registry");

    let changed_old = registry
        .acquire(changed_address.clone(), Arc::clone(&changed_v1), || {
            bound_document_for_snapshot(changed_path, Arc::clone(&changed_v1), &domain)
        })
        .unwrap();
    let stable_old = registry
        .acquire(stable_address.clone(), Arc::clone(&stable), || {
            bound_document_for_snapshot(stable_path, Arc::clone(&stable), &domain)
        })
        .unwrap();
    let old_program = ProgramSnapshot::new(
        vec![
            Arc::clone(changed_old.document()),
            Arc::clone(stable_old.document()),
        ],
        2,
    )
    .unwrap();

    let changed_new = registry
        .update_incrementally(
            &changed_old,
            changed_address,
            Arc::clone(&changed_v2),
            ByteTextChangeRange {
                span: ByteTextSpan::new(edit_start as u32, 1),
                new_length: 1,
            },
            IncrementalDocumentOptions {
                parse: ParseOptions::default(),
                incremental: IncrementalParseOptions {
                    record_reuse_lineage: true,
                },
            },
            &domain,
        )
        .unwrap();
    let stable_new = registry
        .acquire(stable_address, Arc::clone(&stable), || {
            panic!("an unchanged document must reuse its parsed and bound entry")
        })
        .unwrap();
    let new_program = ProgramSnapshot::new(
        vec![
            Arc::clone(changed_new.lease.document()),
            Arc::clone(stable_new.document()),
        ],
        2,
    )
    .unwrap();

    assert!(changed_new.parse_stats.incremental);
    assert!(!changed_new.parse_stats.full_parse_fallback);
    assert!(changed_new.parse_stats.reused_list_elements >= 2);
    assert!(!changed_new.parse_stats.lineage.is_empty());
    assert!(!Arc::ptr_eq(
        old_program.document(0),
        new_program.document(0)
    ));
    assert!(Arc::ptr_eq(
        old_program.document(1),
        new_program.document(1)
    ));
    assert!(Arc::ptr_eq(
        old_program.document(0).source().snapshot(),
        &changed_v1
    ));
    assert!(Arc::ptr_eq(
        new_program.document(0).source().snapshot(),
        &changed_v2
    ));
    assert_eq!(old_program.document(0).source().text(), changed_v1_text);
    assert_eq!(new_program.document(0).source().text(), changed_v2.text());
    assert_eq!(registry.active_entry_count(), 3);

    registry.release(changed_old).unwrap();
    registry.release(stable_old).unwrap();
    registry.release(changed_new.lease).unwrap();
    registry.release(stable_new).unwrap();
    assert_eq!(registry.active_entry_count(), 0);
    assert_eq!(registry.active_reference_count(), 0);

    // Registry release is explicit, while identity leases remain valid for
    // the immutable Programs that still expose the old and new IDs.
    let live = domain.stats().unwrap();
    assert!(live.space(IdentitySpace::Node).active_ranges >= 3);
    drop(old_program);
    drop(new_program);
    let reclaimed = domain.stats().unwrap();
    for space in [
        IdentitySpace::Node,
        IdentitySpace::NodeArray,
        IdentitySpace::Symbol,
        IdentitySpace::PrivateNameSerial,
    ] {
        assert_eq!(reclaimed.space(space).active_ranges, 0, "{space:?}");
    }
}
