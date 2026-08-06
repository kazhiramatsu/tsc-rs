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
