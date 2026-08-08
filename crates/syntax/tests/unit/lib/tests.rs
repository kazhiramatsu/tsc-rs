use super::*;

#[test]
fn parse_source_file_creates_root_and_eof_nodes() {
    let source = parse_source_file("a.ts", "", ParseOptions::default(), None);

    assert_eq!(source.node_count(), 2);
    assert_eq!(source.identifier_count(), 0);
    assert_eq!(source.positions().line_count(), 1);
    assert_eq!(source.positions().line_start_utf16(0), Some(0));
    assert_eq!(source.arena.node(source.root).kind, SyntaxKind::SourceFile);

    let data = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("root is a source file");
    let eof = data.end_of_file_token.expect("source file has EOF token");
    assert_eq!(source.arena.node(eof).kind, SyntaxKind::EndOfFileToken);
    assert_eq!(source.arena.node(eof).parent, Some(source.root));
}

#[test]
fn snapshot_entry_preserves_text_and_position_owner_identity() {
    let snapshot = TextSnapshot::new("const value = '😀';\n", DocumentVersion::new("host-v1"));
    let source = parse_source_file_from_snapshot(
        "a.ts",
        Arc::clone(&snapshot),
        ParseOptions::default(),
        None,
    );

    assert!(Arc::ptr_eq(&snapshot, source.snapshot()));
    assert!(Arc::ptr_eq(
        &snapshot.shared_text(),
        &source.snapshot().shared_text()
    ));
    assert!(Arc::ptr_eq(
        &snapshot.shared_positions(),
        &source.snapshot().shared_positions()
    ));
    assert_eq!(
        source.positions().kind(),
        tsc_diagnostics::PositionIndexKind::StaticDense
    );
}

#[test]
fn detached_arena_preserves_ids_without_extending_published_leases() {
    let mut source = parse_source_file(
        "emit.ts",
        "const value = 1;\n",
        ParseOptions::default(),
        None,
    );
    let domain = IdentityDomain::reclaiming();
    source.relocate_into_identity_domain(&domain).unwrap();
    assert!(source.arena.has_identity_leases());

    let original_node_end = source.arena.node_end();
    let original_array_end = source.arena.array_end();
    let mut detached = source.arena.detached_clone();
    assert!(!detached.has_identity_leases());
    assert_eq!(detached.node_base(), source.arena.node_base());
    assert_eq!(detached.array_base(), source.arena.array_base());
    assert_eq!(detached.node_end(), original_node_end);
    assert_eq!(detached.array_end(), original_array_end);

    let synthetic = detached.alloc_token(
        SyntaxKind::Identifier,
        usize::MAX,
        usize::MAX,
        tsc_types::NodeFlags::SYNTHESIZED,
    );
    assert_eq!(synthetic.0, original_node_end);
    assert!(!source.arena.contains_node(synthetic));
    assert_eq!(source.arena.node_end(), original_node_end);
    assert!(source.arena.has_identity_leases());
}

#[test]
fn generated_relocation_matches_a_direct_forced_nonzero_parse() {
    let text = r#"
/** @typedef {{ value: string }} Alias */
export class Box { #value = 1; method<T>(arg: T) { return [arg, this.#value]; } }
"#;
    let mut local = parse_source_file("forced.ts", text, ParseOptions::default(), None);
    let domain = IdentityDomain::reclaiming();
    let node_gap = domain.lease(IdentitySpace::Node, 11).unwrap();
    let array_gap = domain.lease(IdentitySpace::NodeArray, 7).unwrap();

    local.relocate_into_identity_domain(&domain).unwrap();
    assert_eq!(local.arena.node_base(), 11);
    assert_eq!(local.arena.array_base(), 7);
    assert!(local.identity_owned_by(&domain));

    let direct = parse_source_file(
        "forced.ts",
        text,
        ParseOptions {
            node_id_base: 11,
            node_array_id_base: 7,
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(local, direct);
    assert!(local
        .arena
        .node_ids()
        .all(|id| local.arena.contains_node(id)));

    drop(local);
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        1
    );
    drop(node_gap);
    drop(array_gap);
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        0
    );
}

#[test]
fn source_clones_retain_leases_and_ephemeral_parse_seals_exact_counts() {
    let reclaiming = IdentityDomain::reclaiming();
    let mut source = parse_source_file("clone.ts", "export const x = 1;", Default::default(), None);
    source.relocate_into_identity_domain(&reclaiming).unwrap();
    let retained = source.clone();
    drop(source);
    assert_eq!(
        reclaiming
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        1
    );
    drop(retained);
    assert_eq!(
        reclaiming
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        0
    );

    let ephemeral = IdentityDomain::ephemeral();
    let snapshot = TextSnapshot::new("let y = 2;", DocumentVersion::new("v1"));
    let source = parse_source_file_from_snapshot_in_identity_domain(
        "ephemeral.ts",
        snapshot,
        ParseOptions::default(),
        None,
        &ephemeral,
    )
    .unwrap();
    assert!(source.identity_owned_by(&ephemeral));
    assert_eq!(
        source.node_identity_lease().unwrap().range().len() as usize,
        source.arena.nodes().len()
    );
    assert_eq!(
        source.array_identity_lease().unwrap().range().len() as usize,
        source.arena.node_arrays().len()
    );
}

#[test]
fn incremental_update_reuses_unchanged_list_elements_with_fresh_ids() {
    use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan};

    let before = "const first = 1;\nconst middle = 2;\nconst last = 3;\n";
    let old_snapshot = TextSnapshot::new(before, DocumentVersion::new("1"));
    let domain = IdentityDomain::reclaiming();
    let old = Arc::new(
        parse_source_file_from_snapshot_in_identity_domain(
            "incremental.ts",
            Arc::clone(&old_snapshot),
            ParseOptions::default(),
            None,
            &domain,
        )
        .unwrap(),
    );
    let edit_start = before.find("2;").unwrap() as u32;
    let after = before.replacen("2;", "20;", 1);
    let new_snapshot = TextSnapshot::new(after, DocumentVersion::new("2"));
    let result = update_language_service_source_file_in_identity_domain(
        Arc::clone(&old),
        Arc::clone(&new_snapshot),
        ByteTextChangeRange {
            span: ByteTextSpan::new(edit_start, 1),
            new_length: 2,
        },
        ParseOptions::default(),
        IncrementalParseOptions {
            record_reuse_lineage: true,
        },
        &domain,
    )
    .unwrap();

    assert!(result.stats.incremental);
    assert!(!result.stats.full_parse_fallback);
    assert!(
        result.stats.reused_list_elements >= 2,
        "{:#?}",
        result.stats
    );
    assert!(result.stats.reused_nodes > result.stats.reused_list_elements);
    assert_eq!(result.stats.lineage.len(), result.stats.reused_nodes);
    assert!(Arc::ptr_eq(result.source.snapshot(), &new_snapshot));
    assert_eq!(
        result.source.text(),
        "const first = 1;\nconst middle = 20;\nconst last = 3;\n"
    );
    assert!(result.source.parse_diagnostics.is_empty());
    assert!(result.stats.lineage.iter().all(|lineage| {
        old.arena.contains_node(lineage.old_node)
            && result.source.arena.contains_node(lineage.new_node)
            && lineage.old_node != lineage.new_node
    }));
}

#[test]
fn incremental_update_rejects_an_inexact_change_range() {
    use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan};

    let old_snapshot = TextSnapshot::new("const value = 1;", DocumentVersion::new("1"));
    let old = Arc::new(parse_source_file_from_snapshot(
        "invalid-change.ts",
        old_snapshot,
        ParseOptions::default(),
        None,
    ));
    let new_snapshot = TextSnapshot::new("const value = 2;", DocumentVersion::new("2"));
    let error = update_language_service_source_file(
        old,
        new_snapshot,
        ByteTextChangeRange {
            span: ByteTextSpan::new(0, 1),
            new_length: 1,
        },
        ParseOptions::default(),
        IncrementalParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error, IncrementalParseError::SuffixMismatch);
}
