use tsc_syntax::SyntaxKind;
use tsc_types::{CheckMode, CompilerOptions, TypeData};

use crate::state::test_support::with_program_state;
use crate::{check_program, InputFile};

/// 5.7b review round #2, re-targeted 5.9c: the early/late name
/// collision MERGES per combineSymbolTables → mergeSymbol
/// (PropertyExcludes is None — declaration-type sameness is
/// 2717's check-time job; oracle probe: `{ x: number;
/// [k]: string }` reports ONLY 2717, no duplicate). The unwind
/// concern this test pinned remains pinned: asking the same
/// question twice answers the same table.
#[test]
fn late_binding_merges_early_late_collisions_idempotently() {
    with_program_state(
        &[(
            "a.ts",
            "const k = \"x\";\ntype T = { x: number; [k]: string };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let type_literal = source
                .arena
                .node_ids()
                .find(|&id| tsc_binder::node_util::kind_of(source, id) == SyntaxKind::TypeLiteral)
                .expect("fixture contains a type literal");
            let symbol = state
                .binder
                .node_symbol(type_literal)
                .expect("type literal binds a symbol");
            let first = state
                .get_members_of_symbol(symbol)
                .expect("early/late collisions merge");
            let second = state
                .get_members_of_symbol(symbol)
                .expect("the retry answers the same table");
            assert_eq!(first.get("x").copied(), second.get("x").copied());
            let merged = first.get("x").copied().expect("x survives the merge");
            assert_eq!(
                state.binder.symbol(merged).declarations.len(),
                2,
                "the merged member carries the early AND late declarations"
            );
        },
    );
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

// m4-review A7: the duplicate arm keeps the FIRST table symbol
// (tsc 57680 replaces only the local binding). tsc-probed rows,
// vendored 6.0.3 noLib.

#[test]
fn duplicate_late_bound_member_type_is_first_wins() {
    // The dup arm reports 2733+2718 and the table keeps the FIRST
    // symbol (i.x = number, verified via get_type_of_symbol; the
    // tail assignment itself escapes as a recorded partial, so no
    // assignability row appears either way).
    assert_eq!(
        checked_rows(
            "const k = \"x\" as const;\ninterface I { [k]: number; [k](): void; }\ndeclare const i: I;\nconst n: number = i.x;\n"
        ),
        [(2733, 38, 3), (2718, 51, 3)]
    );
}

#[test]
fn triple_duplicate_late_bound_member_reports_against_the_first() {
    // The third (boolean) declaration merges into and compares
    // against number — the FIRST symbol — for 2717.
    assert_eq!(
        checked_rows(
            "const k = \"x\" as const;\ninterface I { [k]: number; [k](): void; [k]: boolean; }\ndeclare const i: I;\nconst n: number = i.x;\n"
        ),
        [(2733, 38, 3), (2718, 51, 3), (2717, 64, 3)]
    );
}

#[test]
fn late_bound_index_info_includes_sibling_property_types() {
    let text = "declare const k: string;\n\
                type T = { [k]: number; x: string };\n\
                declare const t: T;\n\
                const n: number = t[\"anything\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let source = state.binder.source(0);
        let type_literal = source
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::TypeLiteral)
            .expect("fixture contains a type literal");
        let symbol = state
            .node_symbol(type_literal)
            .expect("type literal carries a symbol");
        let infos = state
            .get_index_infos_of_symbol(symbol)
            .expect("late-bound index info resolves");
        let info = infos
            .iter()
            .find(|info| info.key_type == state.tables.intrinsics.string)
            .expect("string index info is synthesized");
        let TypeData::Union { types, .. } = &state.tables.type_of(info.value_type).data else {
            panic!("sibling property type must join the computed property type");
        };
        assert!(types.contains(&state.tables.intrinsics.number));
        assert!(types.contains(&state.tables.intrinsics.string));

        let access = source
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ElementAccessExpression)
            .expect("fixture contains the indexed access");
        let access_type = state
            .check_expression_cached(access, CheckMode::NORMAL)
            .expect("the indexed access resolves");
        let TypeData::Union { types, .. } = &state.tables.type_of(access_type).data else {
            panic!("the synthesized index info must reach the access consumer");
        };
        assert!(types.contains(&state.tables.intrinsics.number));
        assert!(types.contains(&state.tables.intrinsics.string));
    });
    let result = check_program(
        &[InputFile {
            name: "a.ts".to_owned(),
            text: text.to_owned(),
        }],
        &CompilerOptions::default(),
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 2411));
}
