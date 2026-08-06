use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, TypeData, TypeFlags, TypeId};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), run)
}

fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let annotation: NodeId =
        find_probe_annotation(state.binder.source(0), name).expect("declared var with annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("annotation resolves")
}

fn literal_number(state: &CheckerState, ty: TypeId) -> f64 {
    match &state.tables.type_of(ty).data {
        TypeData::Literal {
            value: tsc_types::LiteralValue::Number(value),
        } => *value,
        other => panic!("expected number literal, got {other:?}"),
    }
}

#[test]
fn enum_declared_type_is_a_stamped_literal_union() {
    with_state(
        "enum E { A, B }\ndeclare var e: E;\ndeclare var a: E.A;\n",
        |state| {
            let e = annotation_type(state, "e");
            let flags = state.tables.flags_of(e);
            // 57466-57469: the member union takes EnumLiteral and
            // the enum symbol.
            assert!(flags.intersects(TypeFlags::UNION));
            assert!(flags.intersects(TypeFlags::ENUM_LITERAL));
            assert!(state.tables.type_of(e).symbol.is_some());
            let TypeData::Union { types, .. } = &state.tables.type_of(e).data else {
                panic!("two-member enums declare unions");
            };
            let members: Vec<TypeId> = types.to_vec();
            assert_eq!(members.len(), 2);
            assert_eq!(literal_number(state, members[0]), 0.0);
            assert_eq!(literal_number(state, members[1]), 1.0);
            // E.A resolves to the member's REGULAR literal type.
            let a = annotation_type(state, "a");
            assert_eq!(a, members[0]);
            assert!(state.tables.flags_of(a).intersects(TypeFlags::ENUM_LITERAL));
        },
    );
}

#[test]
fn enum_values_evaluate_auto_and_constant_expressions() {
    with_state(
        "enum E { A = 3, B, C = (A | B) * 2, D = \"x\" + \"y\", E2 = `a${\"b\"}c` }\n\
         declare var c: E.C;\ndeclare var d: E.D;\n",
        |state| {
            let c = annotation_type(state, "c");
            // A|B = 3|4 = 7, *2 = 14.
            assert_eq!(literal_number(state, c), 14.0);
            let d = annotation_type(state, "d");
            match &state.tables.type_of(d).data {
                TypeData::Literal {
                    value: tsc_types::LiteralValue::String(text),
                } => assert!(text.eq_utf8("xy")),
                other => panic!("expected string literal, got {other:?}"),
            }
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn single_member_enum_declares_the_literal_itself() {
    with_state("enum One { A }\ndeclare var v: One;\n", |state| {
        let one = annotation_type(state, "v");
        let flags = state.tables.flags_of(one);
        // getUnionType over one literal returns the literal — no
        // union to stamp, so the symbol stays the MEMBER's.
        assert!(!flags.intersects(TypeFlags::UNION));
        assert!(flags.intersects(TypeFlags::NUMBER_LITERAL));
        assert!(flags.intersects(TypeFlags::ENUM_LITERAL));
        assert_eq!(literal_number(state, one), 0.0);
    });
}

#[test]
fn ambient_uninitialized_members_get_computed_enum_types() {
    with_state("declare enum A { X }\ndeclare var v: A;\n", |state| {
        let a = annotation_type(state, "v");
        let flags = state.tables.flags_of(a);
        assert!(flags.intersects(TypeFlags::ENUM), "{flags:?}");
        assert!(!flags.intersects(TypeFlags::UNION));
        assert!(matches!(state.tables.type_of(a).data, TypeData::Enum));
    });
}

#[test]
fn computed_enum_names_follow_the_bindable_name_split() {
    with_state(
        "const key = \"member\" as const;\n\
         enum Bound { [key] }\n\
         enum Skipped { [key + \"suffix\"] }\n\
         declare var bound: Bound;\n\
        declare var skipped: Skipped;\n",
        |state| {
            state.check_source_file(0);
            // The entity-name expression is late-bindable and is
            // included through getSymbolOfDeclaration; the binary
            // expression is dynamic but not late-bindable and is
            // skipped by hasBindableName.
            let bound = annotation_type(state, "bound");
            assert_eq!(literal_number(state, bound), 0.0);
            let skipped = annotation_type(state, "skipped");
            assert!(state.tables.flags_of(skipped).intersects(TypeFlags::ENUM));
            assert!(matches!(state.tables.type_of(skipped).data, TypeData::Enum));
            assert_eq!(
                state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect::<Vec<_>>(),
                [1164, 1164]
            );
        },
    );
}

#[test]
fn property_name_text_switch_matches_tsc_kinds() {
    with_program_state(
        &[(
            "a.tsx",
            "const signed = \"-1\";\n\
             const numeric = 42;\n\
             const object = { [\"s\"]: 0, [1]: 0, [1n]: 0, [`t`]: 0 };\n\
             tag`template`;\n\
             const jsx = <ns:name />;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let ids = source.arena.node_ids().collect::<Vec<_>>();
            let string = ids
                .iter()
                .copied()
                .find(|&id| {
                    matches!(&source.arena.node(id).data, NodeData::StringLiteral(data) if data.text == "-1")
                })
                .expect("signed numeric text literal");
            let numeric = ids
                .iter()
                .copied()
                .find(|&id| {
                    matches!(&source.arena.node(id).data, NodeData::NumericLiteral(data) if data.text == "42")
                })
                .expect("numeric literal");
            let bigint = ids
                .iter()
                .copied()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::BigIntLiteral)
                .expect("bigint literal");
            let template = ids
                .iter()
                .copied()
                .find(|&id| {
                    matches!(
                        &source.arena.node(id).data,
                        NodeData::NoSubstitutionTemplateLiteral(data)
                            if data.text == "template"
                    )
                })
                .expect("template literal");
            let jsx_name = ids
                .iter()
                .copied()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::JsxNamespacedName)
                .expect("JSX namespaced name");

            // A signed numeric property name is represented by
            // its textual StringLiteral name; PrefixUnaryExpression
            // is not a PropertyName kind.
            assert_eq!(
                state.try_get_text_of_property_name(string).as_deref(),
                Some("-1")
            );
            assert_eq!(
                state.try_get_text_of_property_name(numeric).as_deref(),
                Some("42")
            );
            assert_eq!(
                state.try_get_text_of_property_name(bigint).as_deref(),
                Some("1n")
            );
            assert_eq!(
                state.try_get_text_of_property_name(template).as_deref(),
                Some("template")
            );
            assert_eq!(
                state.try_get_text_of_property_name(jsx_name).as_deref(),
                Some("ns:name")
            );
            assert_eq!(
                state.try_get_text_of_property_name(source.root),
                None,
                "tryGetTextOfPropertyName's switch default is undefined"
            );

            let computed = ids
                .iter()
                .copied()
                .filter(|&id| source.arena.node(id).kind == SyntaxKind::ComputedPropertyName)
                .map(|name| {
                    let expression = match state.data_of(name) {
                        NodeData::ComputedPropertyName(data) => {
                            data.expression.expect("computed expression")
                        }
                        _ => unreachable!("kind/data agree"),
                    };
                    (
                        state.kind_of(expression),
                        state.try_get_text_of_property_name(name),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                computed,
                [
                    (SyntaxKind::StringLiteral, Some("s".to_owned())),
                    (SyntaxKind::NumericLiteral, Some("1".to_owned())),
                    (SyntaxKind::BigIntLiteral, None),
                    (
                        SyntaxKind::NoSubstitutionTemplateLiteral,
                        Some("t".to_owned())
                    ),
                ]
            );
        },
    );
}

#[test]
fn enum_forward_reference_reports_2651_and_yields_zero() {
    with_state("enum E { A = B, B = 1 }\ndeclare var a: E.A;\n", |state| {
        let a = annotation_type(state, "a");
        assert_eq!(literal_number(state, a), 0.0);
        let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
        assert_eq!(codes, vec![2651]);
    });
}

#[test]
fn enum_self_reference_reports_2565_then_checks_the_initializer_expression() {
    with_state("enum E { A = A }\ndeclare var a: E.A;\n", |state| {
        let annotation = find_probe_annotation(state.binder.source(0), "a")
            .expect("declared var with annotation");
        // The self-reference evaluates to no value, so tsc falls
        // into checkExpression + checkTypeAssignableTo (85654) —
        // live since 5.5e. The member type is number-based, so the
        // assignable check passes and the oracle total is the one
        // 2565 (oracle-pinned 2026-07-13).
        state
            .get_type_from_type_node(annotation)
            .expect("computed enum member checks its initializer since 5.5e");
        let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
        assert_eq!(codes, vec![2565]);
        // Recompute is idempotent (the 2565 dedupes).
        state
            .get_type_from_type_node(annotation)
            .expect("recompute stays clean");
        let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
        assert_eq!(codes, vec![2565]);
    });
}

#[test]
fn enum_member_referencing_earlier_const_evaluates() {
    with_state(
        "const x = 3;\nenum E { A = x, B = First.A + 1 }\nenum First { A = 1 }\n\
         declare var a: E.A;\ndeclare var b: E.B;\n",
        |state| {
            let a = annotation_type(state, "a");
            assert_eq!(literal_number(state, a), 3.0);
            // Cross-enum references force the OTHER enum's values;
            // First is declared after E, which 2651 only forbids
            // for members, not whole enums declared later? No —
            // 2651 covers members declared after the referencing
            // initializer INCLUDING other enums' members, so B
            // reports and evaluates to 0.
            let b = annotation_type(state, "b");
            assert_eq!(literal_number(state, b), 1.0);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, vec![2651]);
        },
    );
}

#[test]
fn enum_relations_route_through_the_enum_relation_cache() {
    with_state(
        "enum E { A, B }\nenum F { A, B }\nconst enum C { A }\n\
         declare var e: E;\ndeclare var f: F;\ndeclare var ea: E.A;\n\
         declare var n: number;\ndeclare var c: C;\n",
        |state| {
            let e = annotation_type(state, "e");
            let f = annotation_type(state, "f");
            let ea = annotation_type(state, "ea");
            let n = annotation_type(state, "n");
            let c = annotation_type(state, "c");
            // Different enums never relate (names differ).
            assert!(!state.is_type_assignable_to(e, f).expect("e->f"));
            assert!(!state.is_type_assignable_to(f, e).expect("f->e"));
            // Members relate to their own enum and to number.
            assert!(state.is_type_assignable_to(ea, e).expect("ea->e"));
            assert!(state.is_type_assignable_to(ea, n).expect("ea->n"));
            assert!(!state.is_type_assignable_to(e, ea).expect("e->ea"));
            // number → numeric enum under assignable (64754-64755).
            assert!(state.is_type_assignable_to(n, e).expect("n->e"));
            assert!(state.is_type_assignable_to(n, ea).expect("n->ea"));
            // const enums still take numbers (Enum flag rules, not
            // RegularEnum): single member C.A is a numeric enum
            // literal.
            assert!(state.is_type_assignable_to(n, c).expect("n->c"));
            assert!(!state.is_type_assignable_to(c, e).expect("c->e"));
        },
    );
}
#[test]
fn tuple_this_append_keeps_the_target() {
    with_state("declare var t: [number, string?];\n", |state| {
        let tuple = annotation_type(state, "t");
        let target = state.tables.reference_target(tuple);
        let with_this = state
            .get_type_with_this_argument(tuple, None, false)
            .expect("tuple-this append is in-slice");
        // tsc 57789 = PLAIN createTypeReference: the SAME tuple
        // target with one extra (this) argument — arity, length
        // and element flags must not change.
        assert_eq!(state.tables.reference_target(with_this), target);
        let arguments = state
            .tables
            .try_type_arguments(with_this)
            .expect("plain references carry resolved arguments")
            .to_vec();
        let TypeData::TupleTarget(data) = &state.tables.type_of(target).data else {
            panic!("tuple annotations target a tuple target");
        };
        assert_eq!(arguments.len(), data.type_parameters.len() + 1);
        assert_eq!(data.element_flags.len(), data.type_parameters.len());
    });
}
