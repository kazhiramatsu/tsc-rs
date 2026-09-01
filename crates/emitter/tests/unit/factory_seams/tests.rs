use super::*;

fn parsed(name: &str, text: &str) -> SourceFile {
    tsc_syntax::parse_source_file(name.to_owned(), text.to_owned(), Default::default(), None)
}

#[test]
fn reuse_clone_accepts_cross_source_original_within_one_arena_only() {
    let first = parsed("first.ts", "export const first = 1;\n");
    let second = parsed("second.ts", "export const second = 2;\n");
    let mut arena = TransformArena::new();
    let first_source = arena.add_source(&first, Some(SourceFileId::from_raw(11)));
    let second_source = arena.add_source(&second, Some(SourceFileId::from_raw(22)));
    let first_root = arena.root(first_source).expect("first root");
    let second_root = arena.root(second_source).expect("second root");

    let reused = arena.factory().clone_node(first_root).expect("reuse clone");
    arena
        .set_original_node(reused, Some(second_root))
        .expect("same-arena cross-source original");
    assert_eq!(
        arena
            .parse_tree_resolver_node(reused)
            .expect("original projection"),
        Some(EmitResolverNode::new(
            SourceFileId::from_raw(22),
            second_root.node(),
        )),
    );

    let rehomed = arena
        .factory()
        .clone_node_to_source(second_root, first_source)
        .expect("same-arena cross-source reuse clone");
    assert_eq!(rehomed.source(), first_source);
    assert_eq!(
        arena
            .parse_tree_resolver_node(rehomed)
            .expect("rehome projection"),
        Some(EmitResolverNode::new(
            SourceFileId::from_raw(22),
            second_root.node(),
        )),
    );

    let mut foreign_arena = TransformArena::new();
    foreign_arena.add_source(&first, None);
    foreign_arena.add_source(&second, None);
    let foreign_source = foreign_arena.add_source(&first, None);
    let foreign = foreign_arena.root(foreign_source).expect("foreign root");
    assert_eq!(
        arena.set_original_node(reused, Some(foreign)),
        Err(TransformError::UnknownSource(foreign_source)),
    );
    assert_eq!(
        arena.factory().clone_node_to_source(foreign, first_source),
        Err(TransformError::UnknownSource(foreign_source)),
    );
}
