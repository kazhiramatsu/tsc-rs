use super::*;

#[test]
fn parse_source_file_creates_root_and_eof_nodes() {
    let source = parse_source_file("a.ts", "", ParseOptions::default(), None);

    assert_eq!(source.node_count(), 2);
    assert_eq!(source.identifier_count(), 0);
    assert_eq!(source.line_map.line_starts, vec![0]);
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
