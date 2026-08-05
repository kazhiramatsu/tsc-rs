use tsc_syntax::SyntaxKind;
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;

#[test]
fn unexpected_scope_owner_drains_by_shape_beside_a_known_block_owner() {
    let text = "export {};\n\
                try {} catch (caught) {}\n\
                if (true) { const local = 1; }\n";
    let options = CompilerOptions {
        no_unused_locals: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("unused-owner.ts", text)], &options, |state| {
        let source = state.binder.source(0);
        let catch = source
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::CatchClause)
            .expect("unexpected but scope-owning recovery canary");
        assert!(
            state.binder.locals_of(catch).is_some(),
            "the fallback must be exercised with a real locals owner"
        );
        let block = source
            .arena
            .node_ids()
            .find(|&node| {
                state.kind_of(node) == SyntaxKind::Block && state.binder.locals_of(node).is_some()
            })
            .expect("known Block sibling with locals");

        state.register_for_unused_identifiers_check(catch);
        state.register_for_unused_identifiers_check(block);
        state.check_registered_unused_identifiers(source.root);

        assert!(
            state.partially_checked_ranges.is_empty(),
            "neither the shape fallback nor the known sibling is partial"
        );
        assert!(
            state
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message_text().contains("'local'")),
            "the known Block sibling still runs its ordinary unused worker"
        );
    });
}
