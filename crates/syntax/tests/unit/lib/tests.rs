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
