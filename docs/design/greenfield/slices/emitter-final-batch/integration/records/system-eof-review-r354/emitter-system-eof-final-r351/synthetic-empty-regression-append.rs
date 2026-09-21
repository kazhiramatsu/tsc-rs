#[test]
fn synthetic_meta_properties_in_an_empty_source_have_no_token_maps() {
    for (keyword_token, name) in [
        (tsc_syntax::SyntaxKind::ImportKeyword, "meta"),
        (tsc_syntax::SyntaxKind::NewKeyword, "target"),
    ] {
        let (_, segments) = print_recorded("", |arena, source, _| {
            let name = arena.factory().create_identifier(source, name).unwrap();
            let meta = arena.factory().create_node(
                source,
                NodeData::MetaProperty(tsc_syntax::nodes::MetaPropertyData {
                    keyword_token,
                    name: Some(name.node()),
                }),
                crate::TransformFlags::NONE,
            ).unwrap();
            let statement = arena.factory().create_node(
                source,
                NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                    expression: Some(meta.node()),
                }),
                crate::TransformFlags::NONE,
            ).unwrap();
            replace_with_statement(arena, source, statement);
        });
        assert!(segments.is_empty(), "{keyword_token:?}: synthetic tokens must not map");
    }
}

