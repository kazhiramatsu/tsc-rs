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
                state.binder.symbol(merged).declarations().len(),
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
    // The dup arm reports TS2300 at both declarations (tsgo; tsc 6.0:
    // 2733+2718) and the table keeps the FIRST
    // symbol (i.x = number, verified via get_type_of_symbol; the
    // tail assignment itself escapes as a recorded partial, so no
    // assignability row appears either way).
    assert_eq!(
        checked_rows(
            "const k = \"x\" as const;\ninterface I { [k]: number; [k](): void; }\ndeclare const i: I;\nconst n: number = i.x;\n"
        ),
        [(2300, 38, 3), (2300, 51, 3)]
    );
}

#[test]
fn triple_duplicate_late_bound_member_reports_against_the_first() {
    // The third (boolean) declaration merges into and compares
    // against number — the FIRST symbol — for 2717. tsgo (tsc-19dadef8)
    // also reports the late-bound conflict as TS2300 'x' at the first two
    // declarations and the merged property as TS2300 '[k]' at all three
    // members named `x` (checkObjectTypeForDuplicateDeclarations).
    assert_eq!(
        checked_rows(
            "const k = \"x\" as const;\ninterface I { [k]: number; [k](): void; [k]: boolean; }\ndeclare const i: I;\nconst n: number = i.x;\n"
        ),
        [
            // Report order: the late-bound rows first, then the member
            // check's (tsgo prints the same rows sorted by position).
            (2300, 38, 3),
            (2300, 51, 3),
            (2300, 38, 3),
            (2300, 51, 3),
            (2300, 64, 3),
            (2717, 64, 3)
        ]
    );
}

#[test]
fn class_self_reference_in_computed_names_keeps_early_static_members_visible() {
    // During resolvedExports late binding, checking `[A.p1]` re-enters the
    // constructor type. The parked early export table must already expose
    // p1/p2, so the only diagnostics are the class-TDZ reports on `A`.
    let text = "class A {\n\
                static readonly p1 = \"x\";\n\
                static readonly p2 = \"y\";\n\
                static readonly [A.p1] = 0;\n\
                static [A.p2]() { return 0 };\n\
                [A.p1]() {}\n\
                [A.p2] = 0;\n\
            }";
    assert_eq!(
        checked_rows(text),
        [(2449, 79, 1), (2449, 98, 1), (2449, 121, 1), (2449, 133, 1)]
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
        assert!(state
            .tables
            .list(*types)
            .contains(&state.tables.intrinsics.number));
        assert!(state
            .tables
            .list(*types)
            .contains(&state.tables.intrinsics.string));

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
        assert!(state
            .tables
            .list(*types)
            .contains(&state.tables.intrinsics.number));
        assert!(state
            .tables
            .list(*types)
            .contains(&state.tables.intrinsics.string));
    });
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), text.to_owned())],
        &CompilerOptions::default(),
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 2411));
}

#[test]
fn late_bound_guard_reads_static_and_instance_links_after_parent_resolution() {
    with_program_state(
        &[(
            "a.ts",
            "declare const key: unique symbol;
           class C { ordinary: number; [key]: string; static [key]: boolean }",
        )],
        &CompilerOptions::default(),
        |state| {
            let class = *state.globals.get("C").unwrap();
            let ordinary = *state
                .binder
                .symbol(class)
                .members()
                .get("ordinary")
                .unwrap();
            assert_eq!(state.get_late_bound_symbol(ordinary), Ok(ordinary));
            let early: Vec<_> = state
                .binder
                .source(0)
                .arena
                .node_ids()
                .filter(|&id| {
                    state.kind_of(id) == SyntaxKind::PropertyDeclaration
                        && state.has_late_bindable_ast_name(id)
                })
                .map(|id| state.node_symbol(id).unwrap())
                .collect();
            assert_eq!(early.len(), 2);
            // A reader of the provisional instance table sees its own symbol.
            let members = state.binder.symbol(class).members().clone();
            state.links.set_symbol_resolved_members_late_bind(
                state.speculation_depth,
                class,
                members,
            );
            assert_eq!(state.get_late_bound_symbol(early[0]), Ok(early[0]));
            assert!(state
                .links
                .symbol_cold()
                .late_symbol
                .get(early[0])
                .is_none());
            state.links.revert_symbol_resolved_members(class);
            for (symbol, is_static) in early.into_iter().zip([false, true]) {
                let late = state.get_late_bound_symbol(symbol).unwrap();
                assert_ne!(late, symbol);
                assert_eq!(state.get_late_bound_symbol(symbol), Ok(late));
                let table = if is_static {
                    state.get_exports_of_symbol(class).unwrap()
                } else {
                    state.get_members_of_symbol(class).unwrap()
                };
                assert_eq!(
                    table.get(state.binder.symbol(late).escaped_name),
                    Some(&late)
                );
            }
        },
    );
}
