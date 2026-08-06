use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, ContextFlags};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

/// The first node of `kind` whose parent satisfies `parent_kind`
/// (None = any parent) — direct get_contextual_type probes; the
/// consuming checkers land in later 5.5 slices.
fn find_node(state: &CheckerState, kind: SyntaxKind, parent_kind: Option<SyntaxKind>) -> NodeId {
    let source = state.binder.source(0);
    source
        .arena
        .node_ids()
        .find(|&id| {
            tsc_binder::node_util::kind_of(source, id) == kind
                && parent_kind.is_none_or(|expected| {
                    tsc_binder::node_util::parent_of(source, id).is_some_and(|parent| {
                        tsc_binder::node_util::kind_of(source, parent) == expected
                    })
                })
        })
        .expect("fixture contains the probe node")
}

#[test]
fn initializer_takes_the_annotation_as_contextual_type() {
    with_program_state(
        &[("a.ts", "let x: number = 1;\n")],
        &CompilerOptions::default(),
        |state| {
            let initializer = find_node(
                state,
                SyntaxKind::NumericLiteral,
                Some(SyntaxKind::VariableDeclaration),
            );
            let contextual = state
                .get_contextual_type(initializer, ContextFlags::NONE)
                .expect("in slice");
            assert_eq!(contextual, Some(state.tables.intrinsics.number));
        },
    );
}

#[test]
fn conditional_operands_inherit_the_outer_context() {
    with_program_state(
        &[(
            "a.ts",
            "declare var b: boolean;\nlet x: number = b ? 1 : 2;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let when_true = find_node(
                state,
                SyntaxKind::NumericLiteral,
                Some(SyntaxKind::ConditionalExpression),
            );
            let contextual = state
                .get_contextual_type(when_true, ContextFlags::NONE)
                .expect("in slice");
            assert_eq!(contextual, Some(state.tables.intrinsics.number));
        },
    );
}

#[test]
fn logical_or_rhs_without_context_takes_the_lhs_type() {
    with_program_state(
        &[("a.ts", "declare var s: string;\ns || \"fallback\";\n")],
        &CompilerOptions::default(),
        |state| {
            let rhs = find_node(
                state,
                SyntaxKind::StringLiteral,
                Some(SyntaxKind::BinaryExpression),
            );
            let contextual = state
                .get_contextual_type(rhs, ContextFlags::NONE)
                .expect("in slice");
            assert_eq!(contextual, Some(state.tables.intrinsics.string));
        },
    );
}

#[test]
fn binding_pattern_initializers_get_the_pattern_type() {
    // No annotation: the SkipBindingPatterns gate builds the type
    // FROM the pattern (includePatternInType) — `a = 1` becomes an
    // optional `a: number` member and the pattern link is stamped.
    with_program_state(
        &[("a.ts", "let { a = 1 } = { a: 2 };\n")],
        &CompilerOptions::default(),
        |state| {
            let literal = find_node(
                state,
                SyntaxKind::ObjectLiteralExpression,
                Some(SyntaxKind::VariableDeclaration),
            );
            let contextual = state
                .get_contextual_type(literal, ContextFlags::NONE)
                .expect("in slice")
                .expect("pattern contextual type");
            let pattern = find_node(state, SyntaxKind::ObjectBindingPattern, None);
            assert_eq!(state.links.ty(contextual).pattern, Some(pattern));
            let member = state
                .get_type_of_property_of_type(contextual, "a")
                .expect("in slice")
                .expect("member a");
            // `a = 1` rides addOptionality: number | undefined
            // under the default strictNullChecks.
            let number = state.tables.intrinsics.number;
            let expected = state.tables.add_optionality(
                number, /*is_property*/ false, /*is_optional*/ true,
            );
            assert_eq!(member, expected);
            // SkipBindingPatterns answers None instead.
            assert_eq!(
                state
                    .get_contextual_type(literal, ContextFlags::SKIP_BINDING_PATTERNS)
                    .expect("in slice"),
                None
            );
        },
    );
}

#[test]
fn contextually_typed_parameter_reads_the_annotated_signature() {
    with_program_state(
        &[(
            "a.ts",
            "let f: (x?: string) => void = function (x = \"a\") {};\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let default_value = find_node(
                state,
                SyntaxKind::StringLiteral,
                Some(SyntaxKind::Parameter),
            );
            let contextual = state
                .get_contextual_type(default_value, ContextFlags::NONE)
                .expect("in slice");
            // `x?: string` reads back as string | undefined under
            // the default strictNullChecks.
            let string = state.tables.intrinsics.string;
            let expected = state.tables.add_optionality(
                string, /*is_property*/ false, /*is_optional*/ true,
            );
            assert_eq!(contextual, Some(expected));
        },
    );
}

#[test]
fn object_literal_discrimination_picks_the_matching_constituent() {
    with_program_state(
        &[(
            "a.ts",
            "interface A { kind: \"a\"; x: number }\ninterface B { kind: \"b\"; y: string }\nlet v: A | B = { kind: \"a\", x: 1 };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let literal = find_node(
                state,
                SyntaxKind::ObjectLiteralExpression,
                Some(SyntaxKind::VariableDeclaration),
            );
            let apparent = state
                .get_apparent_type_of_contextual_type(literal, ContextFlags::NONE)
                .expect("in slice")
                .expect("discriminated type");
            let a_symbol = state
                .resolve_file_scope_name("A", tsc_types::SymbolFlags::INTERFACE)
                .expect("A resolves");
            let a_declared = state
                .get_declared_type_of_class_or_interface(a_symbol)
                .expect("in slice");
            assert_eq!(apparent, a_declared);
        },
    );
}

#[test]
fn fresh_literals_widen_without_a_matching_context() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let fresh = {
            let regular = state.tables.get_string_literal_type("a");
            state.tables.get_fresh_type_of_literal_type(regular)
        };
        let widened = state
            .get_widened_literal_like_type_for_contextual_type(fresh, None)
            .expect("in slice");
        assert_eq!(widened, state.tables.intrinsics.string);
        let regular = state.tables.get_regular_type_of_literal_type(fresh);
        let kept = state
            .get_widened_literal_like_type_for_contextual_type(fresh, Some(regular))
            .expect("in slice");
        assert_eq!(kept, regular);
    });
}

#[test]
fn const_assertion_operands_are_const_contexts() {
    with_program_state(
        &[("a.ts", "let x = \"a\" as const;\n")],
        &CompilerOptions::default(),
        |state| {
            let operand = find_node(
                state,
                SyntaxKind::StringLiteral,
                Some(SyntaxKind::AsExpression),
            );
            assert!(state.is_const_context(operand).expect("in slice"));
        },
    );
}

#[test]
fn generic_mapped_context_substitutes_the_named_property() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends { a: string }>(x: { [K in keyof T]: T[K] }) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let mapped_node = find_node(state, SyntaxKind::MappedType, None);
            let mapped = state
                .get_type_from_type_node(mapped_node)
                .expect("mapped type resolves");
            let property = state
                .get_type_of_property_of_contextual_type(mapped, "a", None)
                .expect("mapped contextual substitution resolves")
                .expect("a is inside the mapped constraint");
            assert!(state
                .is_type_assignable_to(property, state.tables.intrinsics.string)
                .expect("substituted property relates to its constraint"));
        },
    );
}

#[test]
fn circular_mapped_contextual_property_is_not_forced() {
    with_program_state(
        &[(
            "a.ts",
            "type M = { [K in \"a\"]: K };\ndeclare let value: M;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let mapped_node = find_node(state, SyntaxKind::MappedType, None);
            let mapped = state
                .get_type_from_type_node(mapped_node)
                .expect("mapped type resolves");
            let property = state
                .get_property_of_type_full(mapped, "a")
                .expect("members resolve")
                .expect("a exists");
            assert!(state.push_type_resolution(
                crate::state::ResolutionTarget::Symbol(property),
                tsc_types::TypeSystemPropertyName::TYPE,
            ));
            let contextual = state
                .get_type_of_concrete_property_of_contextual_type(mapped, "a")
                .expect("cycle is a non-result, not an unwind");
            assert_eq!(contextual, None);
            state.pop_type_resolution();
        },
    );
}

#[test]
fn computed_getters_borrow_their_bindable_setter_type() {
    let text = "declare const key: unique symbol;\nclass A {\n    get [key]() { return \"\"; }\n    set [key](_alue: number) {}\n}\nconst enum E { Key = 1 }\nclass B {\n    get [E.Key]() { return true; }\n    set [E.Key](_alue: number) {}\n}\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        assert_eq!(
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start,
                        diagnostic.length,
                        diagnostic.message_text(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    2322,
                    Some(62),
                    Some(6),
                    "Type 'string' is not assignable to type 'number'."
                ),
                (
                    2322,
                    Some(164),
                    Some(6),
                    "Type 'boolean' is not assignable to type 'number'."
                )
            ]
        );
    });
}

#[test]
fn jsdoc_getter_type_precedes_the_setter_annotation() {
    let text =
        "class A {\n/** @type {string} */\nget value() { return \"\"; }\nset value(_value) {}\n}\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.js", text)], &options, |state| {
        let getter = find_node(state, SyntaxKind::GetAccessor, None);
        let actual = state
            .get_return_type_from_annotation(getter)
            .expect("JSDoc getter type resolves");
        assert_eq!(actual, Some(state.tables.intrinsics.string));
    });
}
