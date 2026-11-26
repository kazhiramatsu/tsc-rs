use super::*;

#[test]
fn js_to_int32_wraps_like_ecmascript() {
    assert_eq!(js_to_int32(0.0), 0);
    assert_eq!(js_to_int32(-0.0), 0);
    assert_eq!(js_to_int32(3.9), 3);
    assert_eq!(js_to_int32(-3.9), -3);
    assert_eq!(js_to_int32(4294967296.0), 0);
    assert_eq!(js_to_int32(4294967295.0), -1);
    assert_eq!(js_to_int32(2147483648.0), -2147483648);
    assert_eq!(js_to_int32(f64::NAN), 0);
    assert_eq!(js_to_int32(f64::INFINITY), 0);
}

#[test]
fn js_pow_matches_ecmascript_special_cases() {
    assert!(js_pow(1.0, f64::INFINITY).is_nan());
    assert!(js_pow(-1.0, f64::NEG_INFINITY).is_nan());
    assert!(js_pow(2.0, f64::NAN).is_nan());
    assert_eq!(js_pow(f64::NAN, 0.0), 1.0);
    assert_eq!(js_pow(2.0, 10.0), 1024.0);
}

#[test]
fn radix_string_to_number_has_no_u128_ceiling_and_rounds_to_even() {
    assert_eq!(
        js_string_to_number("0x100000000000000000000000000000000"),
        2.0f64.powi(128)
    );
    assert_eq!(
        js_string_to_number(&format!("0b{}", "1".repeat(129))),
        2.0f64.powi(129)
    );
    assert_eq!(
        js_string_to_number(&format!("0o{}", "7".repeat(50))),
        2.0f64.powi(150)
    );
    assert_eq!(js_string_to_number("0x20000000000001"), 2.0f64.powi(53));
    assert_eq!(
        js_string_to_number("0x20000000000003"),
        2.0f64.powi(53) + 4.0
    );
    assert!(js_string_to_number(&format!("0x{}", "f".repeat(256))).is_infinite());
    assert!(js_string_to_number("0x").is_nan());
    assert!(js_string_to_number("0b102").is_nan());
}

#[test]
fn string_to_number_uses_js_whitespace_and_rejects_lone_units() {
    assert_eq!(js_string_to_number("\u{feff}42\u{feff}"), 42.0);
    for text in ["\u{85}42", "42\u{85}", "\u{200b}42"] {
        assert!(js_string_to_number(text).is_nan());
    }
    for unit in [0xd800, 0xd801, 0xdc00] {
        let value = JsString::from_code_units(&[0xfeff, unit, 0xfeff]);
        assert!(js_string_to_number(value.as_js()).is_nan());
    }
}

#[test]
fn numeric_literal_names_round_trip() {
    assert!(is_numeric_literal_name("0"));
    assert!(is_numeric_literal_name("10"));
    assert!(is_numeric_literal_name("1.5"));
    assert!(is_numeric_literal_name("-1"));
    assert!(is_numeric_literal_name("Infinity"));
    assert!(is_numeric_literal_name("NaN"));
    assert!(!is_numeric_literal_name("1.0"));
    assert!(!is_numeric_literal_name("01"));
    assert!(!is_numeric_literal_name("1e2")); // "100" round-trip
    assert!(!is_numeric_literal_name("A"));
    assert!(!is_numeric_literal_name(""));
}

/// The two walks of the declared-before-use test answer as the reference's
/// walks do: the search for an enclosing interface, type alias or type
/// literal (it ends at a node kept from the last search) for every node of a
/// file in three orders, and the same-scope-descendant test (answered from
/// the text ranges when the use lies outside the declaration) for every
/// identifier against every variable declaration.
#[test]
fn declared_before_use_walks_answer_as_the_reference_walks() {
    use crate::state::CheckerState;
    use tsc_syntax::{NodeId, SyntaxKind};

    fn type_node_walk(state: &CheckerState, node: NodeId) -> bool {
        if state
            .binder
            .flags_of(node)
            .intersects(tsc_types::NodeFlags::AMBIENT)
        {
            return true;
        }
        let mut current = Some(node);
        while let Some(node) = current {
            if matches!(
                state.kind_of(node),
                SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::TypeLiteral
            ) {
                return true;
            }
            current = state.parent_of(node);
        }
        false
    }

    let text = "declare const k: unique symbol;\n\
        interface I { a: number; [k]: string; m(x: typeof v): void }\n\
        type A = { b: I; [k]: number } | typeof v;\n\
        declare namespace Amb { const z: number; function g(): typeof z; }\n\
        const v = 1, w = v + ((): number => w)();\n\
        function f(p = v) { const a = p, b = a + (() => b)(); \
        { let c = a; for (const d of [c, d]) { c = d; } return (x: { y: typeof c }) => x.y; } }\n\
        class C { p = v; m() { for (let i = 0, j = i; i < j; i++) { const q = i; \
        if (q) { return q; } } return this.p; } }\n\
        for (const e in { e }) { e; }\n";
    crate::state::test_support::with_program_state(
        &[("a.ts", text)],
        &tsc_types::CompilerOptions::default(),
        |state| {
            let nodes: Vec<NodeId> = state.binder.source(0).arena.node_ids().collect();
            let backward: Vec<NodeId> = nodes.iter().rev().copied().collect();
            let striding: Vec<NodeId> = (0..nodes.len())
                .map(|index| nodes[index * 7919 % nodes.len()])
                .collect();
            let (mut in_type, mut outside_type) = (0, 0);
            for order in [&nodes, &backward, &striding] {
                for &node in order {
                    let expected = type_node_walk(state, node);
                    assert_eq!(
                        state.is_in_ambient_or_type_node(node),
                        expected,
                        "{node:?} ({:?})",
                        state.kind_of(node)
                    );
                    if expected {
                        in_type += 1;
                    } else {
                        outside_type += 1;
                    }
                }
            }
            assert!(in_type >= 3 * 30, "{in_type}");
            assert!(outside_type >= 3 * 60, "{outside_type}");

            let declarations: Vec<NodeId> = nodes
                .iter()
                .copied()
                .filter(|&node| state.kind_of(node) == SyntaxKind::VariableDeclaration)
                .collect();
            let identifiers: Vec<NodeId> = nodes
                .iter()
                .copied()
                .filter(|&node| state.kind_of(node) == SyntaxKind::Identifier)
                .collect();
            let (mut inside, mut outside) = (0, 0);
            for &declaration in &declarations {
                let stop_at = state.get_enclosing_block_scope_container(declaration);
                for &identifier in &identifiers {
                    let expected = state
                        .walk_same_scope_descendent_of(identifier, declaration, stop_at)
                        .unwrap();
                    assert_eq!(
                        state
                            .is_same_scope_descendent_of(identifier, Some(declaration), stop_at)
                            .unwrap(),
                        expected,
                        "{identifier:?} under {declaration:?}"
                    );
                    if expected {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }
            // Every declaration holds its own name; most identifiers lie
            // outside most declarations.
            assert!(declarations.len() >= 10, "{}", declarations.len());
            assert!(inside >= declarations.len(), "{inside}");
            assert!(outside > 10 * inside, "{outside} against {inside}");
        },
    );
}
