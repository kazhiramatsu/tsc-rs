use tsc_types::{CompilerOptions, SymbolFlags, TypeData, TypeFlags};

use crate::state::test_support::with_program_state;

/// 5.7b review round: the unique-symbol type identity contract —
/// one type per declaration (SymbolLinks.uniqueESSymbolType memo),
/// UNIQUE_ES_SYMBOL flagged, distinct across declarations, and
/// widening collapses to the plain `symbol` intrinsic.
#[test]
fn unique_symbol_types_are_per_declaration_memoized_and_widen() {
    with_program_state(
        &[(
            "a.ts",
            "declare const u: unique symbol;\ndeclare const v: unique symbol;\nlet l: unique symbol;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let sym = |state: &mut crate::state::CheckerState, name: &str| {
                state
                    .get_global_symbol(name, SymbolFlags::VALUE, None)
                    .expect("fixture declares the name")
            };
            let u = sym(state, "u");
            let v = sym(state, "v");
            let u_type = state.get_type_of_symbol(u).expect("u types");
            let v_type = state.get_type_of_symbol(v).expect("v types");
            assert!(state
                .tables
                .flags_of(u_type)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL));
            assert!(state
                .tables
                .flags_of(v_type)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL));
            assert_ne!(
                u_type, v_type,
                "distinct declarations mint distinct unique types"
            );
            let name_of = |state: &crate::state::CheckerState, ty| {
                match &state.tables.type_of(ty).data {
                    TypeData::UniqueESSymbol { escaped_name } => escaped_name.clone(),
                    other => panic!("expected a unique symbol, got {other:?}"),
                }
            };
            let u_name = name_of(state, u_type);
            let v_name = name_of(state, v_type);
            assert!(u_name.starts_with("__@u@"), "{u_name}");
            assert!(v_name.starts_with("__@v@"), "{v_name}");
            assert_ne!(u_name, v_name);
            // The per-declaration memo: re-resolving the same
            // declaration answers the SAME TypeId.
            let u_decl = state.binder.symbol(u).declarations[0];
            let first = state
                .get_es_symbol_like_type_for_node(u_decl)
                .expect("resolves");
            let second = state
                .get_es_symbol_like_type_for_node(u_decl)
                .expect("resolves");
            assert_eq!(first, second, "SymbolLinks.uniqueESSymbolType memoizes");
            assert_eq!(first, u_type);
            // An INVALID position (a `let`) answers the plain
            // `symbol` intrinsic, not a unique type.
            let l = sym(state, "l");
            let l_decl = state.binder.symbol(l).declarations[0];
            let l_type = state
                .get_es_symbol_like_type_for_node(l_decl)
                .expect("resolves");
            assert_eq!(l_type, state.tables.intrinsics.es_symbol);
            // Widening collapses unique → symbol.
            let widened = state
                .get_widened_unique_es_symbol_type(u_type)
                .expect("widens");
            assert_eq!(widened, state.tables.intrinsics.es_symbol);
        },
    );
}
