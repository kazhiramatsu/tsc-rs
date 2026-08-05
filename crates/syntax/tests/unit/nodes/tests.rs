use super::*;

#[test]
fn generated_node_schema_has_core_nodes() {
    assert_eq!(NodeData::Token.kind(), None);
    let _ = IdentifierData {
        escaped_text: String::new(),
        text: String::new(),
    };
    assert_eq!(
        NodeData::missing(SyntaxKind::Identifier).kind(),
        Some(SyntaxKind::Identifier)
    );
    assert_eq!(NodeData::missing(SyntaxKind::SemicolonToken).kind(), None);
}
