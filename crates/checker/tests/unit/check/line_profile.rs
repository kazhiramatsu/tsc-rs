use super::*;
use crate::state::test_support::with_program_state;
use tsc_syntax::SyntaxKind;
use tsc_types::CompilerOptions;

#[test]
fn line_profile_attributes_nested_and_deferred_checks() {
    let run = |enabled| {
        with_program_state(
            &[("a.ts", "declare function f(): number;\n{\n  f();\n}\n")],
            &CompilerOptions::default(),
            |state| {
                state.line_profile.enabled = enabled;
                let source = state.binder.source(0);
                let call = source
                    .arena
                    .node_ids()
                    .find(|&node| source.arena.node(node).kind == SyntaxKind::CallExpression)
                    .unwrap();
                state.check_node_deferred(call);
                state.check_source_file(0);
                let profile = &state.line_profile;
                assert!(profile.stack.is_empty());
                if enabled {
                    let file = source as *const SourceFile as usize;
                    let block = &profile.rows[&(file, 1)];
                    let statement = &profile.rows[&(file, 2)];
                    assert!(block.inclusive_ops[OP_SIGNATURES] > block.self_ops[OP_SIGNATURES]);
                    assert!(statement.hits >= 2);
                    assert!(statement.inclusive_ops[OP_SIGNATURES] > 0);
                } else {
                    assert!(profile.files.is_empty() && profile.rows.is_empty());
                }
                (
                    state.profile_ops,
                    state.total_instantiation_count,
                    state.tables.len(),
                    state.expression_checks,
                )
            },
        )
    };
    assert_eq!(run(false), run(true));
}
