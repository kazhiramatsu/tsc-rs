use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

/// Driver-level fixture check (check.rs idiom): full
/// check_source_file, checker-sink rows as (code, start, length).
fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    checked_rows_with(text, &CompilerOptions::default())
}

fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        rows(state)
    })
}

fn checked_js_rows(text: &str) -> Vec<(u32, u32, u32)> {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_any: Some(true),
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        rows(state)
    })
}

#[test]
fn require_binding_elements_are_alias_declarations_but_ordinary_bindings_are_not() {
    // isAliasSymbolDeclaration 48498-48500 owns the BindingElement
    // predicate independently of the binder's Alias flag.
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "a.js",
            "const { K } = require('./mod');\n\
             const object = { K: 1 };\n\
             const { K: local } = object;\n",
        )],
        &options,
        |state| {
            let source = state.binder.source(0);
            let elements = source
                .arena
                .node_ids()
                .filter(|&node| {
                    tsc_binder::node_util::kind_of(source, node) == SyntaxKind::BindingElement
                })
                .collect::<Vec<_>>();
            assert_eq!(elements.len(), 2);
            assert!(state.is_alias_symbol_declaration(elements[0]));
            assert!(!state.is_alias_symbol_declaration(elements[1]));
        },
    );
}

#[test]
fn checked_js_alias_declaration_predicates_match_tsc_boundaries() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "a.js",
            "function C() {}\n\
             let x, y;\n\
             exports = module.exports = C;\n\
             exports = module.exports = 1;\n\
             x = y = C;\n\
             const object = { C, literal: 1, arrow: () => 1, alias: C };\n\
             exports.plain = function plain() {};\n\
             exports.ctor = /** @constructor */ function Ctor() {};\n",
        )],
        &options,
        |state| {
            let source = state.binder.source(0);

            let module_exports = source
                .arena
                .node_ids()
                .filter(|&node| {
                    tsc_binder::node_util::kind_of(source, node) == SyntaxKind::BinaryExpression
                        && tsc_binder::get_assignment_declaration_kind(source, node)
                            == tsc_binder::AssignmentDeclarationKind::ModuleExports
                })
                .collect::<Vec<_>>();
            assert_eq!(module_exports.len(), 2);
            assert!(state.is_alias_symbol_declaration(module_exports[0]));
            assert!(!state.is_alias_symbol_declaration(module_exports[1]));

            let ordinary_assignments = source
                .arena
                .node_ids()
                .filter(|&node| match state.data_of(node) {
                    NodeData::BinaryExpression(data) => data.left.is_some_and(|left| {
                        state
                            .identifier_text_of(left)
                            .is_some_and(|name| matches!(name, "x" | "y"))
                    }),
                    _ => false,
                })
                .collect::<Vec<_>>();
            assert_eq!(ordinary_assignments.len(), 2);
            assert!(ordinary_assignments
                .into_iter()
                .all(|node| !state.is_alias_symbol_declaration(node)));

            let shorthand = source
                .arena
                .node_ids()
                .find(|&node| state.kind_of(node) == SyntaxKind::ShorthandPropertyAssignment)
                .expect("fixture shorthand");
            assert!(state.is_alias_symbol_declaration(shorthand));

            let property_aliases = source
                .arena
                .node_ids()
                .filter_map(|node| {
                    (state.kind_of(node) == SyntaxKind::PropertyAssignment)
                        .then(|| {
                            state
                                .name_of_node(node)
                                .and_then(|name| state.identifier_text_of(name))
                                .map(|name| (name, node))
                        })
                        .flatten()
                })
                .collect::<std::collections::HashMap<_, _>>();
            assert!(!state.is_alias_symbol_declaration(property_aliases["literal"]));
            assert!(!state.is_alias_symbol_declaration(property_aliases["arrow"]));
            assert!(state.is_alias_symbol_declaration(property_aliases["alias"]));

            let access_aliases = source
                .arena
                .node_ids()
                .filter_map(|node| {
                    matches!(
                        state.kind_of(node),
                        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                    )
                    .then(|| {
                        state
                            .name_of_node(node)
                            .and_then(|name| state.identifier_text_of(name))
                            .map(|name| (name, node))
                    })
                    .flatten()
                })
                .collect::<std::collections::HashMap<_, _>>();
            assert!(!state.is_alias_symbol_declaration(access_aliases["plain"]));
            assert!(state.is_alias_symbol_declaration(access_aliases["ctor"]));
        },
    );
}

#[test]
fn export_assignment_aliases_exclude_function_expressions() {
    with_program_state(
        &[(
            "a.ts",
            "declare const C: unknown;\n\
             export = function () {};\n\
             export = class {};\n\
             export = C;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let assignments = source
                .arena
                .node_ids()
                .filter(|&node| state.kind_of(node) == SyntaxKind::ExportAssignment)
                .collect::<Vec<_>>();
            assert_eq!(assignments.len(), 3);
            assert!(!state.is_alias_symbol_declaration(assignments[0]));
            assert!(state.is_alias_symbol_declaration(assignments[1]));
            assert!(state.is_alias_symbol_declaration(assignments[2]));
        },
    );
}

#[test]
fn ambient_const_enum_access_reports_under_isolated_module_options() {
    let text = "declare const enum F { A }\nF.A;\n";
    let start = text.find("F.A").expect("fixture access") as u32;
    for options in [
        CompilerOptions {
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            isolated_modules: Some(true),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    ] {
        assert_eq!(checked_rows_with(text, &options), [(2748, start, 1)]);
    }
}

fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
    state
        .diagnostics
        .iter()
        // File-less program diagnostics (lazy missing-global 2318s
        // in no-lib fixtures) are excluded from per-file output.
        .filter(|diag| diag.file_name.is_some())
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

fn super_ordering_rows(text: &str) -> Vec<(u32, u32, u32)> {
    checked_rows(text)
        .into_iter()
        .filter(|(code, _, _)| matches!(code, 17009 | 17011))
        .collect()
}

/// The first node of `kind` whose parent satisfies `parent_kind`
/// (None = any parent) — for direct check_expression probes of
/// arms the 5.5a driver cannot reach (assignment operands route
/// through the 5.5e binary arm, super through 5.5d receivers).
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

fn direct_expression_rows(
    text: &str,
    kind: SyntaxKind,
    parent: Option<SyntaxKind>,
) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let node = find_node(state, kind, parent);
        let _ = state.check_expression(node, CheckMode::NORMAL);
        rows(state)
    })
}

// ---- driver forcing (checkExpressionStatement) — oracle-pinned ----

#[test]
fn this_and_super_property_before_super_call_are_reported() {
    let text =
        "class B {}\nclass C extends B { constructor() { this.x; super.x; super(); } x = 0; }\n";
    assert_eq!(super_ordering_rows(text), [(17009, 47, 4), (17011, 55, 5)]);
}

#[test]
fn super_ordering_requires_every_reaching_branch_and_ignores_extends_null() {
    assert_eq!(
        super_ordering_rows(
            "class B {}\nclass C extends B { constructor(ok: boolean) { if (ok) super(); this.x; } x = 0; }\n"
        ),
        [(17009, 75, 4)]
    );
    assert_eq!(
        super_ordering_rows(
            "class B {}\nclass C extends B { constructor(ok: boolean) { if (ok) super(); else super(); this.x; super.x; } x = 0; }\n"
        ),
        []
    );
    assert_eq!(
        super_ordering_rows(
            "class C extends null { constructor() { this.x; super.x; super(); } x = 0; }\n"
        ),
        []
    );
}

#[test]
fn expression_statements_force_identifier_resolution() {
    assert_eq!(checked_rows("missingName;\n"), [(2304, 0, 11)]);
}

#[test]
fn resolved_identifiers_are_silent() {
    assert_eq!(checked_rows("let y: number = 1;\ny;\n"), []);
}

#[test]
fn export_default_identifier_flows_before_its_declaration() {
    assert_eq!(
        checked_rows("export default x;\nconst x = 'x';\n"),
        [(2448, 15, 1), (2454, 15, 1)]
    );
}

#[test]
fn typeof_forces_its_operand() {
    assert_eq!(checked_rows("typeof missing;\n"), [(2304, 7, 7)]);
}

#[test]
fn void_defers_then_checks_its_operand() {
    // checkVoidExpression registers the node; the deferred drain
    // checks the operand (checkDeferredNode's void arm).
    assert_eq!(checked_rows("void missing;\n"), [(2304, 5, 7)]);
}

#[test]
fn delete_of_a_non_access_reports_2703() {
    // Oracle also reports 1102 (strict-mode delete on an
    // identifier) from the BINDER sink — merged at the program
    // layer, not part of the checker sink this asserts.
    assert_eq!(
        checked_rows("declare var x: number;\ndelete x;\n"),
        [(2703, 30, 1)]
    );
}

// ---- literals — oracle-pinned ----

#[test]
fn literal_statements_are_silent() {
    assert_eq!(
        checked_rows("\"abc\";\n123;\ntrue;\nfalse;\nnull;\n1n;\n"),
        []
    );
    // Regex forces the RegExp global: no-lib fixtures take the
    // one-shot file-less 2318, excluded from per-file rows.
    assert_eq!(checked_rows("/abc/;\n"), []);
}

#[test]
fn bigint_below_es2020_reports_2737() {
    let options = CompilerOptions {
        target: Some(tsc_types::ScriptTarget::ES5.bits()),
        ..CompilerOptions::default()
    };
    assert_eq!(checked_rows_with("1n;\n", &options), [(2737, 0, 2)]);
}

#[test]
fn hex_bigint_literals_convert_to_base10() {
    let parsed = super::parse_pseudo_big_int("0x10n").expect("hex bigint parses");
    assert_eq!(parsed.base10_value, "16");
    assert!(!parsed.negative);
    let parsed = super::parse_pseudo_big_int("0b1010n").expect("binary bigint parses");
    assert_eq!(parsed.base10_value, "10");
    let parsed = super::parse_pseudo_big_int("0o777n").expect("octal bigint parses");
    assert_eq!(parsed.base10_value, "511");
    let parsed =
        super::parse_pseudo_big_int("0xffffffffffffffffn").expect("wide hex bigint parses");
    assert_eq!(parsed.base10_value, "18446744073709551615");
    let parsed = super::parse_pseudo_big_int("000123n").expect("decimal strips zeros");
    assert_eq!(parsed.base10_value, "123");
}

#[test]
fn scanner_invalid_bigint_text_recovers_as_zero() {
    for text in ["", "not-a-bigint", "0xn", "0b2n", "0o8n", "0x_n", "0x10"] {
        let parsed = super::parse_pseudo_big_int(text).expect("invalid scanner text recovers");
        assert_eq!(parsed.base10_value, "0", "{text:?}");
        assert!(!parsed.negative);
    }
}

// ---- TDZ (checkResolvedBlockScopedVariable) — oracle-pinned ----

#[test]
fn let_used_before_declaration_reports_2448_with_related() {
    with_program_state(
        &[("a.ts", "x;\nlet x: number = 1;\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            // 2454 rides along since 6.2 (the flipped initialType
            // ladder + real assignment arm) — oracle parity.
            assert_eq!(rows(state), [(2448, 0, 1), (2454, 0, 1)]);
            let diag = &state.diagnostics[0];
            assert_eq!(diag.related.len(), 1);
            assert_eq!(diag.related[0].message.code, 2728);
            assert_eq!(diag.related[0].start, Some(7));
            assert_eq!(diag.related[0].length, Some(1));
        },
    );
}

#[test]
fn class_used_before_declaration_reports_2449() {
    assert_eq!(checked_rows("C;\nclass C {}\n"), [(2449, 0, 1)]);
}

#[test]
fn enum_used_before_declaration_reports_2450() {
    assert_eq!(checked_rows("E;\nenum E { A }\n"), [(2450, 0, 1)]);
}

#[test]
fn function_wrapped_tdz_use_is_legal() {
    // isUsedInFunctionOrInstanceProperty: a non-IIFE function
    // defers the use past the declaration.
    assert_eq!(
        checked_rows("function g(): void { x; }\nlet x: number = 1;\n"),
        []
    );
}

#[test]
fn var_used_before_declaration_is_not_tdz() {
    // No TDZ for var; the oracle's 2454 fires since 6.2 (flipped
    // initialType ladder + real assignment arm).
    assert_eq!(checked_rows("v;\nvar v: number;\n"), [(2454, 0, 1)]);
}

// ---- ambient statement grammar — oracle-pinned ----

#[test]
fn statements_in_ambient_contexts_report_1036_once_per_block() {
    // The DRIVER cannot reach ambient statements until
    // checkModuleDeclaration lands (5.8) — namespace bodies are an
    // honest FN band; this drives check_source_element directly at
    // the statements to pin the grammar port (oracle spans).
    let direct = |text: &str, count: usize| {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            let source = state.binder.source(0);
            let statements: Vec<NodeId> = source
                .arena
                .node_ids()
                .filter(|&id| {
                    tsc_binder::node_util::kind_of(source, id) == SyntaxKind::ExpressionStatement
                })
                .collect();
            assert_eq!(statements.len(), count);
            for statement in statements {
                state.check_source_element(Some(statement));
            }
            rows(state)
        })
    };
    assert_eq!(direct("declare namespace N { 1; }\n", 1), [(1036, 22, 1)]);
    // Once-flag sits on the enclosing block: a second statement
    // stays silent.
    assert_eq!(
        direct("declare namespace N { 1; 2; }\n", 2),
        [(1036, 22, 1)]
    );
}

// ---- this / super — oracle-pinned ----

#[test]
fn this_in_namespace_body_reports_2331() {
    // Driver reachability + the VALUE_MODULE getTypeOfSymbol arm
    // (both 5.8d): 2331 plus the noImplicitThis implicit-any 2683
    // — the full oracle pair.
    let rows = direct_expression_rows(
        "namespace N { this; }\nexport {};\n",
        SyntaxKind::ThisKeyword,
        None,
    );
    assert_eq!(rows, [(2331, 14, 4), (2683, 14, 4)]);
}

#[test]
fn super_without_extends_reports_2335() {
    let rows = direct_expression_rows(
        "class A { m(): void { super.x; } }\n",
        SyntaxKind::SuperKeyword,
        None,
    );
    assert_eq!(rows, [(2335, 22, 5)]);
}

#[test]
fn super_outside_class_members_reports_2660() {
    let rows = direct_expression_rows(
        "function g(): void { super.x; }\n",
        SyntaxKind::SuperKeyword,
        None,
    );
    assert_eq!(rows, [(2660, 21, 5)]);
}

// ---- assignment-target mutability — oracle-pinned (direct
// probes: assignments route through the 5.5e binary arm, so the
// driver cannot reach these until the trampoline lands) ----

fn assignment_lhs_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let source = state.binder.source(0);
        let node = source
            .arena
            .node_ids()
            .find(|&id| {
                tsc_binder::node_util::kind_of(source, id) == SyntaxKind::Identifier
                    && tsc_binder::node_util::parent_of(source, id).is_some_and(|parent| {
                        matches!(
                            &source.arena.node(parent).data,
                            NodeData::BinaryExpression(data) if data.left == Some(id)
                        )
                    })
            })
            .expect("fixture contains an assignment LHS identifier");
        let _ = state.check_expression(node, CheckMode::NORMAL);
        rows(state)
    })
}

#[test]
fn assigning_to_an_enum_reports_2628() {
    assert_eq!(
        assignment_lhs_rows("enum E { A }\nE = 1;\n"),
        [(2628, 13, 1)]
    );
}

#[test]
fn assigning_to_a_class_reports_2629() {
    assert_eq!(assignment_lhs_rows("class C {}\nC = 1;\n"), [(2629, 11, 1)]);
}

#[test]
fn assigning_to_a_function_reports_2630() {
    assert_eq!(
        assignment_lhs_rows("function f(): void {}\nf = 1;\n"),
        [(2630, 22, 1)]
    );
}

#[test]
fn assigning_to_a_const_reports_2588() {
    assert_eq!(
        assignment_lhs_rows("const c: number = 1;\nc = 2;\n"),
        [(2588, 21, 1)]
    );
}

// ---- onFailedToResolveSymbol chain (5.5a slice) — oracle-pinned ----

#[test]
fn primitive_type_name_in_value_position_reports_2693() {
    assert_eq!(checked_rows("string;\n"), [(2693, 0, 6)]);
}

#[test]
fn instance_member_near_miss_reports_2663() {
    // Method bodies are driver-unreachable until 5.8 — direct
    // probe on the body identifier.
    let rows = direct_expression_rows(
        "class C { foo: number = 1; m(): void { foo; } }\n",
        SyntaxKind::Identifier,
        Some(SyntaxKind::ExpressionStatement),
    );
    assert_eq!(rows, [(2663, 39, 3)]);
}

#[test]
fn static_member_near_miss_reports_2662() {
    let rows = direct_expression_rows(
        "class C { static bar: number = 1; m(): void { bar; } }\n",
        SyntaxKind::Identifier,
        Some(SyntaxKind::ExpressionStatement),
    );
    assert_eq!(rows, [(2662, 46, 3)]);
}

#[test]
fn primitive_name_inside_class_without_member_reports_2693() {
    let rows = direct_expression_rows(
        "class C { m(): void { string; } }\n",
        SyntaxKind::Identifier,
        Some(SyntaxKind::ExpressionStatement),
    );
    assert_eq!(rows, [(2693, 22, 6)]);
}

// ---- per-element containment ----

#[test]
fn out_of_slice_expressions_abandon_only_their_statement() {
    // Statement 2's binary WORKER arm is a 5.5e stub, but the
    // trampoline checks both operands first — the operand's 2304
    // (oracle-exact) lands before the escape contains the rest of
    // the statement. Statement 3 still checks.
    assert_eq!(
        checked_rows("let a: number = 1;\na + missingName;\nmissingName2;\n"),
        [(2304, 23, 11), (2304, 36, 12)]
    );
}

#[test]
fn rechecking_is_idempotent() {
    with_program_state(
        &[("a.ts", "missingName;\nE;\nenum E { A }\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let first = rows(state);
            assert_eq!(first, [(2304, 0, 11), (2450, 13, 1)]);
            state.check_source_file(0);
            assert_eq!(rows(state), first);
        },
    );
}

// ---- 5.5 seam residuals — oracle-pinned ----

#[test]
fn unresolved_type_names_type_as_error_type() {
    // resolveEntityName reports the 2304s; typeof/type references
    // then type as errorType, so the property reads stay silent
    // (the old escapes contained both statements after the 2304).
    assert_eq!(
        checked_rows(
            "declare const x: typeof missingV;\nx.prop;\ndeclare const y: MissingT;\ny.prop;\n"
        ),
        [(2304, 24, 8), (2304, 59, 8)]
    );
}

#[test]
fn delete_of_non_optional_property_reports_2790() {
    // checkDeleteExpressionMustBeOptional: strict default → the
    // non-optional operand reports 2790; the optional one is
    // clean (hasTypeFacts IsUndefined sees the added undefined).
    assert_eq!(
        checked_rows(
            "declare const o: { p: number };\ndelete o.p;\ndeclare const q: { p?: number };\ndelete q.p;\n"
        ),
        [(2790, 39, 3)]
    );
}

#[test]
fn mixin_factory_extends_expression_checks() {
    // check_base_type_expression = plain checkExpression (tsc
    // 57156): the mixin-call base resolves and instances type.
    assert_eq!(
        checked_rows(
            "declare function Mix(): new () => { m: number };\nclass C extends Mix() { }\ndeclare const c: C;\nc.m.bad;\n"
        ),
        [(2339, 99, 3)]
    );
}

#[test]
fn computed_property_name_flows_into_keyof() {
    // getLiteralTypeFromPropertyName's computed arm rides
    // checkComputedPropertyName: keyof typeof o = "kk" (object-
    // literal members carry the computed-name declaration).
    assert_eq!(
        checked_rows(
            "const k = \"kk\";\nconst o = { [k]: 1 };\ntype K = keyof typeof o;\ndeclare const kk: K;\ndeclare function take(x: \"nope\"): void;\ntake(kk);\n"
        ),
        [(2345, 129, 2)]
    );
}

#[test]
fn type_literal_computed_member_late_binds() {
    // lateBindMember (57662) landed with the 5.7b review round:
    // `{ [k]: number }` resolves its computed member, keyof O =
    // "kk", and the oracle row (2345 at `kk`) goes live.
    assert_eq!(
        checked_rows(
            "const k = \"kk\";\ntype O = { [k]: number };\ntype K = keyof O;\ndeclare const kk: K;\ndeclare function take(x: \"nope\"): void;\ntake(kk);\n"
        ),
        [(2345, 126, 2)]
    );
}

// ---- getQuickTypeOfExpression call/await arms — oracle-pinned ----

#[test]
fn quick_call_initializer_types_the_variable() {
    // Call arm: a single non-generic signature types the
    // initializer without argument checks (oracle rows exactly).
    assert_eq!(
        checked_rows("declare function f(): number;\nconst x = f();\nx.bad;\n"),
        [(2339, 47, 3)]
    );
}

#[test]
fn quick_call_chain_initializer_keeps_undefined() {
    // Chain flavor rides getReturnTypeOfSingleNonGenericSignature-
    // OfCallChain: the optional marker propagates into `y`. The
    // demand is an argument check (2345, live since 5.7a; the
    // assignment face is live too since the 6.6f gate
    // retirement).
    assert_eq!(
        checked_rows(
            "declare const g: (() => number) | undefined;\nconst y = g?.();\ndeclare function take(n: number): void;\ntake(y);\n"
        ),
        [(2345, 107, 1)]
    );
}

#[test]
fn higher_order_generic_argument_lifts_type_parameters() {
    // 7.4c FRONTIER pin, FLIPPED at M6 7.5 to its oracle face
    // (probed 2026-07-20 probe74i.mjs and re-probed 2026-07-21
    // probe75.mjs, vendored 6.0.3 noLib — tsc: f is the lifted
    // generic `<T>(a: T) => {}` and `n` reports [(2322, 148, 1)],
    // args '{}' vs 'number' from the noLib Array miss).
    // pipe(list, list) drives the 80767-80815 higher-order path:
    // pass-1 defers via SkipGenericFunctions, the re-run lifts
    // `list`'s T (getUniqueTypeParameters + mergeInferences +
    // inferredTypeParameters -> chooseOverload 76844), and the
    // applicability walk now runs compareSignaturesRelated's
    // generic-source arm (64505-64514) live through the
    // frame-loaned iSICO.
    assert_eq!(
        checked_rows(
            "declare function pipe<A, B, C>(f: (a: A) => B, g: (b: B) => C): (a: A) => C;\ndeclare function list<T>(x: T): T[];\nconst f = pipe(list, list);\nconst n: number = f(1);\n"
        ),
        [(2322, 148, 1)]
    );
}

#[test]
fn quick_call_generic_initializer_resolves_live() {
    // LIVE since 7.4b (the stub era contained this element and the
    // 2339 was a recorded FN): inference types y as '1', so `.bad`
    // reports 2339 @52 exactly as the oracle. Oracle-pinned
    // 2026-07-20 (scratchpad probe74.mjs, vendored 6.0.3 noLib).
    assert_eq!(
        checked_rows("declare function id<T>(x: T): T;\nconst y = id(1);\ny.bad;\n"),
        [(2339, 52, 3)]
    );
}

#[test]
fn quick_call_require_guard_falls_through() {
    // isRequireCall guard: the quick path declines and the full
    // resolution reports the noLib require spelling row (the `r;`
    // statement supplies the type demand).
    assert_eq!(
        checked_rows("const r = require(\"m\");\nr;\n"),
        [(2591, 10, 7)]
    );
}

#[test]
fn jsdoc_type_assertion_is_checked_and_drives_quick_type() {
    let text = "const value = /** @type {string} */ (1);\nvalue.missing;\n";
    let mut actual = checked_js_rows(text);
    actual.sort_by_key(|row| row.1);
    assert_eq!(actual, [(2352, 25, 6), (2339, 47, 7)]);
}

#[test]
fn jsdoc_type_tag_signature_supplies_this_type() {
    let text = "/** @type {function(this: { x: number }): void} */\n\
                function f() { this.x = \"bad\"; }\n";
    assert!(checked_js_rows(text)
        .into_iter()
        .any(|(code, _, _)| code == 2322));
}

#[test]
fn compound_assignment_rhs_inherits_jsdoc_this_tag() {
    let text = "const holder = {};\n\
                /** @this {{ x: number }} */\n\
                holder.m ??= function () { this.x = \"bad\"; };\n";
    assert!(checked_js_rows(text)
        .into_iter()
        .any(|(code, _, _)| code == 2322));
}

#[test]
fn commonjs_exported_expression_preserves_literal_type() {
    let text = "exports.value = \"x\";\n\
                /** @param {\"x\"} value */\n\
                function take(value) {}\n\
                take(exports.value);\n\
                let local = \"x\";\n\
                take(local);\n";
    let actual = checked_js_rows(text)
        .into_iter()
        .filter(|row| row.0 == 2345)
        .collect::<Vec<_>>();
    assert_eq!(actual, [(2345, 114, 5)]);
}

#[test]
fn commonjs_source_file_this_uses_module_export_type() {
    let actual = checked_js_rows("exports.x = 1;\nthis.x.bad;\n")
        .into_iter()
        .filter(|row| row.0 == 2339)
        .collect::<Vec<_>>();
    assert_eq!(actual, [(2339, 22, 3)]);
}

#[test]
fn js_constructor_prototype_method_uses_the_instance_this_type() {
    // The raw checker sink also contains the noLib `prototype`
    // lookup row. The load-bearing assertion is the 2322 inside
    // the assigned method: `this` has C's instance type.
    assert_eq!(
        checked_js_rows(
            "function C() { this.x = 0; }\nC.prototype.m = function () { this.x = 'bad'; };\n"
        ),
        [(2339, 31, 9), (2322, 59, 6)]
    );
}

#[test]
fn homomorphic_mapped_type_inherits_const_type_parameter() {
    with_program_state(
        &[(
            "a.ts",
            "function f<const T>(value: { [K in keyof T]: T[K] }) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let mapped_node = find_node(state, SyntaxKind::MappedType, None);
            let mapped = state
                .get_type_from_type_node(mapped_node)
                .expect("mapped type resolves");
            assert!(state
                .is_const_type_variable(Some(mapped), 0)
                .expect("mapped constness resolves"));
        },
    );
}
