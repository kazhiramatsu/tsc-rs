use tsc_binder::Binder;
use tsc_syntax::{parse_source_file, NodeData, NodeId, ParseOptions, SourceFile, SyntaxKind};
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

/// Driver-level fixture check (access.rs idiom): oracle-pinned
/// rows (tsc 6.0.3, noLib, options {} unless stated) — scratchpad
/// ops{1,2,3}.ts probes, 2026-07-13.
// ---- elaboration rows (6.6 review; oracle-pinned vs vendored
// tsc 6.0.3 noLib, 2026-07-19) ----

#[test]
fn computed_nonliteral_key_member_row_reports_2418() {
    // isComputedNonLiteralName selects the 2418 message (64449).
    assert_eq!(
        checked_rows("const k = \"a\";\nconst t: { a: number } = { [k]: \"s\" };\n"),
        [(2418, 42, 3)]
    );
}

#[test]
fn paren_wrapped_initializer_elaborates_the_member_row() {
    // elaborateError's ParenthesizedExpression arm (63975).
    assert_eq!(
        checked_rows("const x: { a: number } = ({ a: \"s\" });\n"),
        [(2322, 28, 1)]
    );
}

#[test]
fn array_element_deep_first_elaboration_anchors_inner_member() {
    // generateLimitedTupleElements feeds the element back through
    // elaborateError (64406-64407) — the row lands on `b`.
    assert_eq!(
        checked_rows("const t: [{ b: number }] = [{ b: \"s\" }];\n"),
        [(2322, 30, 1)]
    );
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        rows(state)
    })
}

/// Build the binder/checker after a test-only arena mutation. The
/// parser itself uses missing nodes for these recoveries, so an
/// absent required slot must be synthesized explicitly to pin the
/// checker-side containment contract.
fn with_synthetic_recovery_state<R>(
    text: &str,
    mutate: impl FnOnce(&mut SourceFile) -> Vec<NodeId>,
    run: impl FnOnce(&mut CheckerState, &[NodeId]) -> R,
) -> R {
    let options = CompilerOptions::default();
    let mut source = parse_source_file(
        "a.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            script_target: options.emit_script_target(),
            ..ParseOptions::default()
        },
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "synthetic recovery fixture must start from a valid sibling tree"
    );
    let nodes = mutate(&mut source);
    let mut binder = Binder::with_bases(&source, &options, 1, 0);
    binder.bind_source_file();
    let mut state = CheckerState::new(&source, &binder, &options);
    state.merge_module_augmentations();
    run(&mut state, &nodes)
}

fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
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
}

#[test]
fn synthetic_missing_operator_slots_are_error_typed_and_siblings_stay_valid() {
    with_synthetic_recovery_state(
        "1 + 2;\n3 + 4;\n-1;\n-2;\ntrue ? 1 : 2;\nfalse ? 3 : 4;\n",
        |source| {
            let binaries: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::BinaryExpression)
                .collect();
            let prefixes: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::PrefixUnaryExpression)
                .collect();
            let conditionals: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::ConditionalExpression)
                .collect();
            assert_eq!(binaries.len(), 2);
            assert_eq!(prefixes.len(), 2);
            assert_eq!(conditionals.len(), 2);
            let NodeData::BinaryExpression(data) = &mut source.arena.node_mut(binaries[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.left = None;
            let NodeData::PrefixUnaryExpression(data) =
                &mut source.arena.node_mut(prefixes[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.operand = None;
            let NodeData::ConditionalExpression(data) =
                &mut source.arena.node_mut(conditionals[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.when_false = None;
            vec![
                binaries[0],
                binaries[1],
                prefixes[0],
                prefixes[1],
                conditionals[0],
                conditionals[1],
            ]
        },
        |state, nodes| {
            let error = state.tables.intrinsics.error;
            assert_eq!(
                state
                    .check_binary_expression(nodes[0], tsc_types::CheckMode::NORMAL)
                    .unwrap(),
                error
            );
            assert_ne!(
                state
                    .check_binary_expression(nodes[1], tsc_types::CheckMode::NORMAL)
                    .unwrap(),
                error
            );
            assert_eq!(
                state.check_prefix_unary_expression(nodes[2]).unwrap(),
                error
            );
            assert_ne!(
                state.check_prefix_unary_expression(nodes[3]).unwrap(),
                error
            );
            assert_eq!(
                state.get_syntactic_nullishness_semantics(nodes[4]).unwrap(),
                super::SEMANTICS_SOMETIMES
            );
            assert_eq!(
                state.get_syntactic_truthy_semantics(nodes[4]).unwrap(),
                super::SEMANTICS_SOMETIMES
            );
            assert_eq!(
                state
                    .check_conditional_expression(nodes[4], tsc_types::CheckMode::NORMAL,)
                    .unwrap(),
                error
            );
            assert_ne!(
                state
                    .check_conditional_expression(nodes[5], tsc_types::CheckMode::NORMAL,)
                    .unwrap(),
                error
            );
        },
    );
}

#[test]
fn synthetic_missing_assertion_and_template_slots_preserve_valid_siblings() {
    with_synthetic_recovery_state(
        "1 as number;\n\
         2 as number;\n\
         1 satisfies number;\n\
         2 satisfies number;\n\
         `a${1}b`;\n\
         `c${2}d`;\n\
         `e${3}f`;\n\
         `g${4}h`;\n",
        |source| {
            let assertions: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::AsExpression)
                .collect();
            let satisfies: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::SatisfiesExpression)
                .collect();
            let templates: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::TemplateExpression)
                .collect();
            let spans: Vec<_> = source
                .arena
                .node_ids()
                .filter(|&node| source.arena.node(node).kind == SyntaxKind::TemplateSpan)
                .collect();
            assert_eq!(assertions.len(), 2);
            assert_eq!(satisfies.len(), 2);
            assert_eq!(templates.len(), 4);
            assert_eq!(spans.len(), 4);
            let NodeData::AsExpression(data) = &mut source.arena.node_mut(assertions[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.expression = None;
            let NodeData::SatisfiesExpression(data) = &mut source.arena.node_mut(satisfies[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.r#type = None;
            let NodeData::TemplateExpression(data) = &mut source.arena.node_mut(templates[0]).data
            else {
                unreachable!("selected by kind")
            };
            data.head = None;
            let NodeData::TemplateSpan(data) = &mut source.arena.node_mut(spans[1]).data else {
                unreachable!("selected by kind")
            };
            data.expression = None;
            let NodeData::TemplateSpan(data) = &mut source.arena.node_mut(spans[2]).data else {
                unreachable!("selected by kind")
            };
            data.literal = None;
            vec![
                assertions[0],
                assertions[1],
                satisfies[0],
                satisfies[1],
                templates[0],
                templates[1],
                templates[2],
                templates[3],
            ]
        },
        |state, nodes| {
            let error = state.tables.intrinsics.error;
            assert_eq!(
                state
                    .check_assertion(nodes[0], tsc_types::CheckMode::NORMAL)
                    .unwrap(),
                error
            );
            assert_ne!(
                state
                    .check_assertion(nodes[1], tsc_types::CheckMode::NORMAL)
                    .unwrap(),
                error
            );
            assert_eq!(state.check_satisfies_expression(nodes[2]).unwrap(), error);
            assert_ne!(state.check_satisfies_expression(nodes[3]).unwrap(), error);
            for &template in &nodes[4..7] {
                assert_eq!(state.check_template_expression(template).unwrap(), error);
            }
            assert_ne!(state.check_template_expression(nodes[7]).unwrap(), error);
        },
    );
}

#[test]
fn awaited_thenable_with_incompatible_this_appends_2684_detail() {
    let text = "interface EPromise<E, A> {\n\
                    e: E;\n\
                    then<B>(this: EPromise<never, A>, onfulfilled?: (value: A) => B): B;\n\
                }\n\
                declare const value: EPromise<number, string>;\n\
                async function f() { await value; }\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1320)
            .expect("invalid await operand");
        assert_eq!(
            diagnostic.message.text,
            "Type of 'await' operand must either be a valid promise or must not contain a callable 'then' member."
        );
        assert_eq!(diagnostic.message.next.len(), 1);
        assert_eq!(diagnostic.message.next[0].code, 2684);
        assert_eq!(
            diagnostic.message.next[0].text,
            "The 'this' context of type 'EPromise<number, string>' is not assignable to method's 'this' of type 'EPromise<never, string>'."
        );
        assert!(diagnostic.message.next[0].next.is_empty());
        assert!(diagnostic.related.is_empty());
    });
}

#[test]
fn awaited_thenable_without_this_rejection_keeps_1320_head_only() {
    let text = "declare const value: { then(onfulfilled: number): void };\n\
                async function f() { await value; }\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1320)
            .expect("invalid await operand");
        assert!(diagnostic.message.next.is_empty());
        assert!(diagnostic.related.is_empty());
    });
}

// ---- arithmetic / + / relational / equality arms ----

#[test]
fn arithmetic_lhs_string_reports_2362_on_the_operand() {
    assert_eq!(
        checked_rows("declare const s0: string;\ns0 * 2;\n"),
        [(2362, 26, 2)]
    );
}

#[test]
fn arithmetic_rhs_string_reports_2363_on_the_operand() {
    assert_eq!(
        checked_rows("declare const s0: string;\n2 * s0;\n"),
        [(2363, 30, 2)]
    );
}

#[test]
fn boolean_bar_suggests_barbar_2447() {
    assert_eq!(
        checked_rows("declare const b0: boolean;\nb0 | b0;\n"),
        [(2447, 27, 7)]
    );
}

#[test]
fn boolean_caret_suggests_strict_inequality_2447() {
    assert_eq!(
        checked_rows("declare const b0: boolean;\nfalse ^ b0;\n"),
        [(2447, 27, 10)]
    );
}

#[test]
fn relational_string_number_reports_2365_on_the_binary() {
    assert_eq!(
        checked_rows("declare const s0: string;\ndeclare const n0: number;\ns0 < n0;\n"),
        [(2365, 52, 7)]
    );
}

#[test]
fn equality_disjoint_primitives_upgrade_to_2367() {
    assert_eq!(
        checked_rows("declare const s0: string;\ndeclare const n0: number;\nn0 === s0;\n"),
        [(2367, 52, 9)]
    );
}

#[test]
fn assignment_mismatch_reports_2322_on_the_left() {
    assert_eq!(
        checked_rows("declare let ln: number;\ndeclare const s0: string;\nln = s0;\n"),
        [(2322, 50, 2)]
    );
}

#[test]
fn cross_file_js_expando_assignment_uses_the_merged_symbol() {
    let rows = with_program_state(
        &[
            ("file1.js", "var N = {};\nN.commands = {};\n"),
            (
                "file2.js",
                "N.commands.a = 111;\nN.commands.b = function () {};\n",
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            state.check_source_file(1);
            rows(state)
        },
    );
    assert_eq!(rows, []);
}

#[test]
fn literal_assignment_target_reports_2364() {
    assert_eq!(checked_rows("1 = 2;\n"), [(2364, 0, 1)]);
}

#[test]
fn unused_comma_left_reports_2695() {
    assert_eq!(
        checked_rows("declare const n0: number;\nn0, 2;\n"),
        [(2695, 26, 2)]
    );
}

#[test]
fn indirect_call_comma_left_reports_2695_unless_access_or_eval() {
    // `(0, f)()` with a PLAIN identifier right is NOT the
    // isIndirectCall exemption shape (80296: access expression or
    // `eval` only) — oracle-pinned 2695 at the `0`. Pre-5.7 the
    // call stub's containment swallowed the row; 5.7a un-escapes
    // the statement and the row renders.
    assert_eq!(
        checked_rows("declare function f(): void;\n(0, f)();\n"),
        [(2695, 29, 1)]
    );
    // The access-expression right IS exempt (oracle: clean).
    assert_eq!(
        checked_rows("declare const o: { f(): void };\n(0, o.f)();\n"),
        []
    );
}

#[test]
fn symbol_arithmetic_takes_2362_not_2469() {
    // The arithmetic band has NO symbol arm — the operand check
    // reports 2362 (oracle-pinned surprise). Plain `symbol`:
    // unique-symbol consts still escape (M4 residual).
    assert_eq!(
        checked_rows("declare const sy: symbol;\nsy * 1;\n"),
        [(2362, 26, 2)]
    );
}

#[test]
fn symbol_plus_number_reports_2365() {
    assert_eq!(
        checked_rows("declare const sy: symbol;\ndeclare const n0: number;\nn0 + sy;\n"),
        [(2365, 52, 7)]
    );
}

#[test]
fn unary_plus_on_symbol_reports_2469() {
    assert_eq!(
        checked_rows("declare const sy: symbol;\n+sy;\n"),
        [(2469, 27, 2)]
    );
}

#[test]
fn unary_plus_on_bigint_reports_2736() {
    assert_eq!(
        checked_rows("declare const bg: bigint;\n+bg;\n"),
        [(2736, 27, 2)]
    );
}

#[test]
fn mixed_bigint_number_arithmetic_reports_2365() {
    assert_eq!(
        checked_rows("declare const bg: bigint;\ndeclare const n0: number;\nbg * n0;\n"),
        [(2365, 52, 7)]
    );
}

#[test]
fn bigint_shift_pair_is_clean() {
    assert_eq!(
        checked_rows("declare let lb: bigint;\ndeclare const bg: bigint;\nlb << bg;\n"),
        []
    );
}

// ---- NaN / shift-simplification ----

#[test]
fn nan_equality_reports_2845_when_global_nan_resolves() {
    // The script-level ambient const IS the global NaN here.
    assert_eq!(
        checked_rows("declare const NaN: number;\ndeclare const n0: number;\nn0 === NaN;\n"),
        [(2845, 53, 10)]
    );
}

#[test]
fn enum_member_shift_of_32_or_more_elevates_6807_to_error() {
    // Oracle: (6807, 14, 7) — checkEnumMember (85810) drives the
    // initializer's expression check since 5.8c.
    assert_eq!(checked_rows("enum SH { X = 1 << 33 }\n"), [(6807, 14, 7)]);
}

#[test]
fn statement_level_shift_simplification_stays_a_suggestion() {
    // errorOrSuggestion's suggestion flavor is unmodeled — the
    // oracle reports a suggestion-band 6807 here, we stay silent.
    assert_eq!(checked_rows("1 << 33;\n"), []);
}

#[test]
fn nan_shift_amount_in_enum_member_stays_silent() {
    // m4-review OP-4: tsc's guard is `Math.abs(value) >= 32`
    // (80100), which is FALSE for NaN — `1 << (0/0)` reports no
    // 6807 (probed clean vs vendored 6.0.3).
    assert_eq!(checked_rows("enum SH2 { X = 1 << (0/0) }\n"), []);
}

// ---- logical / nullish bands ----

#[test]
fn void_condition_reports_1345() {
    assert_eq!(
        checked_rows("declare const vv: void;\nvv && 1;\n"),
        [(1345, 24, 2)]
    );
}

#[test]
fn always_truthy_literal_condition_reports_2872() {
    assert_eq!(checked_rows("2 && 1;\n"), [(2872, 0, 1)]);
}

#[test]
fn checked_js_syntactic_truthiness_row_is_published() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "function f() { return 5 || true; }\n".to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diag| (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2872, 22, 1)]
    );
}

#[test]
fn checked_js_non_jsdoc_operator_row_is_published() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "const orbitol = 1;\nvar orbitol = 1 + false;\n".to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diag| (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2451, 6, 7), (2451, 23, 7), (2365, 33, 9)]
    );
}

/// Oracle pin (tsc 6.0.3, nodeModulesAllowJsImportMeta.ts,
/// 2026-07-26): the CommonJS package file reports TS1470 while
/// its ES-module package sibling remains clean.
#[test]
fn checked_js_import_meta_commonjs_row_is_published() {
    let source = "const x = import.meta.url;\nexport {x};\n";
    let result = check_program(
        &[
            InputFile {
                name: "/package.json".to_owned(),
                text: "{\"type\":\"module\"}\n".to_owned(),
            },
            InputFile {
                name: "/index.js".to_owned(),
                text: source.to_owned(),
            },
            InputFile {
                name: "/subfolder/package.json".to_owned(),
                text: "{\"type\":\"commonjs\"}\n".to_owned(),
            },
            InputFile {
                name: "/subfolder/index.js".to_owned(),
                text: source.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(100),
            target: Some(9),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1470)
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [(
            Some("/subfolder/index.js"),
            1470,
            Some(source.find("import.meta").expect("import.meta") as u32),
            Some("import.meta".len() as u32),
            "The 'import.meta' meta-property is not allowed in files which will build into CommonJS output.",
        )]
    );
}

#[test]
fn zero_and_one_literals_are_sometimes_truthy() {
    assert_eq!(checked_rows("0 && 1;\n1 && 0;\n"), []);
}

#[test]
fn empty_string_condition_reports_2873() {
    assert_eq!(checked_rows("\"\" && 1;\n"), [(2873, 0, 2)]);
}

#[test]
fn literal_null_coalesce_left_reports_2871() {
    assert_eq!(checked_rows("null ?? 1;\n"), [(2871, 0, 4)]);
}

#[test]
fn nullable_typed_identifier_is_syntactically_sometimes_nullish() {
    assert_eq!(checked_rows("declare const nu: null;\nnu ?? 5;\n"), []);
}

#[test]
fn mixed_coalesce_and_logical_report_5076_both_ways() {
    assert_eq!(
        checked_rows(
            "declare const za: number | null;\ndeclare const zb: number;\nza ?? zb || zb;\nzb || za ?? zb;\n"
        ),
        [(5076, 59, 8), (5076, 75, 8)]
    );
}

#[test]
fn always_defined_function_condition_reports_2774() {
    // Function-TYPED const: function DECLARATION symbols still
    // escape (signature declaration kind, 5.6/5.8).
    assert_eq!(
        checked_rows("declare const ff: () => void;\nff && 1;\n"),
        [(2774, 30, 2)]
    );
}

#[test]
fn bare_identifier_right_operand_does_not_suppress_2774() {
    // forEachChild(right) sees only CHILDREN — a bare `ff` right
    // operand has none, so the report stands (oracle-pinned).
    assert_eq!(
        checked_rows("declare const ff: () => void;\nff && ff;\n"),
        [(2774, 30, 2)]
    );
}

#[test]
fn function_condition_used_in_chain_suppresses_2774() {
    // The suppression walk sees `ff` inside the right operand's
    // call; the call itself then escapes (5.7) with nothing
    // emitted — matching the oracle's clean verdict.
    assert_eq!(
        checked_rows("declare const ff: () => void;\nff && ff();\n"),
        []
    );
}

#[test]
fn coalesce_result_is_subtype_union_of_nonnullable_left_and_right() {
    // `du ?? 3` : string | 3 (Subtype keeps the disjoint literal).
    assert_eq!(
        checked_rows(
            "declare let du: string | undefined;\ndeclare let mn: number;\nmn = du ?? 3;\n"
        ),
        [(2322, 60, 2)]
    );
}

#[test]
fn coalesce_result_subtype_reduction_absorbs_matching_literal() {
    assert_eq!(
        checked_rows(
            "declare let du: string | undefined;\ndeclare let ms: string;\nms = du ?? \"z\";\n"
        ),
        []
    );
}

// ---- unary increment/decrement selection ----

#[test]
fn mutable_string_increment_reports_2356() {
    assert_eq!(
        checked_rows("declare let ms: string;\nms++;\n"),
        [(2356, 24, 2)]
    );
}

#[test]
fn const_string_increment_reports_only_2588() {
    // The const-assignment 2588 fires inside checkExpression and
    // degrades the operand to errorType — the arithmetic check
    // then passes silently (oracle-pinned surprise).
    assert_eq!(
        checked_rows("declare const s0: string;\ns0++;\n"),
        [(2588, 26, 2)]
    );
}

#[test]
fn parenthesized_literal_increment_reports_2357() {
    assert_eq!(checked_rows("(1)++;\n"), [(2357, 0, 3)]);
}

#[test]
fn prefix_decrement_of_boolean_literal_reports_2356_only() {
    assert_eq!(checked_rows("--true;\n"), [(2356, 2, 4)]);
}

// ---- destructuring assignment ----

#[test]
fn tuple_destructuring_mismatches_report_2322_per_element() {
    assert_eq!(
        checked_rows(
            "declare const tup: [number, string];\ndeclare let mn: number;\ndeclare let ms: string;\n[ms, mn] = tup;\n"
        ),
        [(2322, 86, 2), (2322, 90, 2)]
    );
}

#[test]
fn tuple_destructuring_out_of_bounds_reports_both_rows() {
    // Oracle: (2322, 119, 2) + (2493, 119, 2) — the 2493 args
    // render the tuple ('[number, string]'); flipped live at
    // phase-9 9.3a (tuple renderer). Port insertion order: the
    // element read emits 2493 during the destructured-type walk,
    // the element assignment then emits 2322.
    assert_eq!(
        checked_rows(
            "declare const tup: [number, string];\ndeclare let mn: number;\ndeclare let ms: string;\ndeclare let mb: boolean;\n[mn, ms, mb] = tup;\n"
        ),
        [(2493, 119, 2), (2322, 119, 2)]
    );
}

#[test]
fn non_last_rest_element_reports_2462() {
    assert_eq!(
        checked_rows(
            "declare const tup: [number, string];\ndeclare let mn: number;\ndeclare let ms: string;\n[...mn, ms] = tup;\n"
        ),
        [(2462, 86, 5)]
    );
}

#[test]
fn nested_tuple_rest_slice_is_clean() {
    assert_eq!(
        checked_rows(
            "declare const tup: [number, string];\ndeclare let mn: number;\ndeclare let ms: string;\n[mn, ...[ms]] = tup;\n"
        ),
        []
    );
}

#[test]
fn object_destructuring_missing_property_reports_2339_on_the_name() {
    // Interface receiver — an inline `{ x: number }` receiver
    // contains on the anonymous display (5.5d gotcha).
    assert_eq!(
        checked_rows(
            "interface O0 { x: number }\ndeclare const obj4: O0;\ndeclare let mn: number;\n({ z: mn } = obj4);\n"
        ),
        [(2339, 78, 1)]
    );
}

#[test]
fn object_destructuring_mismatch_reports_2322_on_the_target() {
    assert_eq!(
        checked_rows(
            "declare const obj0: { x: number };\ndeclare let ms: string;\n({ x: ms } = obj0);\n"
        ),
        [(2322, 65, 2)]
    );
}

#[test]
fn object_rest_destructuring_with_matching_target_is_clean() {
    assert_eq!(
        checked_rows(
            "declare const obj0: { x: number, y: string };\ndeclare let mn: number;\ndeclare let rest0: { y: string };\n({ x: mn, ...rest0 } = obj0);\n"
        ),
        []
    );
}

#[test]
fn destructuring_defaults_strip_undefined_and_stay_clean() {
    assert_eq!(
        checked_rows(
            "declare const tup: [number, string];\ndeclare let mn: number;\ndeclare let ms: string;\n[mn = 1, ms = \"a\"] = tup;\n"
        ),
        []
    );
}

// ---- assertions / satisfies / instantiation ----

#[test]
fn string_as_number_reports_2352() {
    assert_eq!(
        checked_rows("declare const s1: string;\ns1 as number;\n"),
        [(2352, 26, 12)]
    );
}

#[test]
fn literal_as_other_literal_is_clean_via_base_widening() {
    // getBaseTypeOfLiteralType strips the top-level literal before
    // the comparable gate: `1 as 2` / `"a" as "b"` are CLEAN
    // (risk #2 matrix).
    assert_eq!(
        checked_rows("const a2 = 1 as 2;\nconst a3 = \"a\" as \"b\";\n"),
        []
    );
}

#[test]
fn object_literal_assertion_mismatch_reports_2352_with_literal_faces() {
    // Oracle: "Conversion of type '{ a: number; }' to type
    // '{ a: string; }' may be a mistake..." — the 9.3b
    // anonymous-object display renders both faces (pre-9.3b the
    // row contained on the curtain).
    assert_eq!(
        checked_rows("const a4 = { a: 1 } as { a: string };\n"),
        [(2352, 11, 25)]
    );
}

#[test]
fn identifier_const_assertion_reports_1355() {
    assert_eq!(
        checked_rows("declare let v4: number;\nv4 as const;\n"),
        [(1355, 24, 2)]
    );
}

#[test]
fn literal_and_enum_member_const_assertions_are_clean() {
    assert_eq!(
        checked_rows("declare enum EC { A = 1 }\n5 as const;\n(5) as const;\nEC.A as const;\n"),
        []
    );
}

#[test]
fn satisfies_mismatch_reports_1360() {
    assert_eq!(
        checked_rows("declare const ns: number;\nns satisfies string;\n"),
        [(1360, 29, 9)]
    );
}

#[test]
fn instantiation_expression_arity_mismatch_renders_2635() {
    // 9.3b2 signature rung: (2635, 37, 14) displaying
    // '<T>(x: T) => T', the span = the type-argument list
    // (oracle-probed; the pre-rung containment note here
    // mis-remembered the span as 34).
    assert_eq!(
        checked_rows("declare const gf: <T>(x: T) => T;\ngf<string, number>;\n"),
        [(2635, 37, 14)]
    );
}

#[test]
fn instantiation_expression_with_correct_arity_is_clean() {
    assert_eq!(
        checked_rows("declare const gf: <T>(x: T) => T;\ngf<string>;\n"),
        []
    );
}

// ---- instanceof / in ----

#[test]
fn primitive_instanceof_lhs_reports_2358() {
    assert_eq!(
        checked_rows("declare const oo: { a: number };\n1 instanceof oo;\n"),
        [(2358, 33, 1)]
    );
}

#[test]
fn object_instanceof_rhs_without_function_shape_is_nolib_degenerate() {
    // noLib: globalFunctionType degenerates and the subtype test
    // passes — the oracle is CLEAN here too (2359 is a lib-loaded
    // row; the conformance gate covers it).
    assert_eq!(
        checked_rows(
            "declare const oo: { a: number };\ndeclare const eo: {};\neo instanceof oo;\n"
        ),
        []
    );
}

#[test]
fn has_instance_first_argument_mismatch_reports_2860() {
    // 5.7b: the resolveCall failure ladder under the 2860 head —
    // the hand-declared SymbolConstructor recreates the
    // known-symbol name path under noLib (oracle-probed u1.ts,
    // 2026-07-13: 2860 at `w`).
    assert_eq!(
        checked_rows(
            "interface SymbolConstructor { readonly hasInstance: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ndeclare const H: { [Symbol.hasInstance](value: { n: number }): boolean };\ndeclare const w: { m: string };\nw instanceof H;\n"
        ),
        [(2860, 214, 1)]
    );
}

#[test]
fn has_instance_non_boolean_return_reports_2861() {
    // 5.7b: checkInstanceOfExpression's boolean check on the
    // resolved signature's return type (oracle-probed u2.ts:
    // 2861 at `H`).
    assert_eq!(
        checked_rows(
            "interface SymbolConstructor { readonly hasInstance: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ndeclare const H: { [Symbol.hasInstance](value: object): number };\ndeclare const o: { x: number };\no instanceof H;\n"
        ),
        [(2861, 219, 1)]
    );
}

#[test]
fn in_rhs_primitive_reports_2322_against_object() {
    assert_eq!(checked_rows("\"a\" in 1;\n"), [(2322, 7, 1)]);
}

#[test]
fn in_with_object_operands_is_clean() {
    assert_eq!(
        checked_rows("declare const oo: { a: number };\n\"a\" in oo;\n"),
        []
    );
}

// ---- meta / template ----

#[test]
fn top_level_new_target_reports_17013() {
    assert_eq!(checked_rows("new.target;\n"), [(17013, 0, 10)]);
}

#[test]
fn new_target_inside_function_is_clean() {
    assert_eq!(checked_rows("function ntf() { new.target; }\n"), []);
}

#[test]
fn template_evaluation_produces_a_fresh_string_literal() {
    // The evaluated literal generalizes to 'string' in the report
    // (reportRelationError literal-source widening, oracle ops5).
    assert_eq!(
        checked_rows("declare let mn: number;\nmn = `a${1}b`;\n"),
        [(2322, 24, 2)]
    );
}

#[test]
fn symbol_template_span_reports_2731() {
    assert_eq!(
        checked_rows("declare const sy2: symbol;\n`${sy2}`;\n"),
        [(2731, 30, 3)]
    );
}

#[test]
fn single_missing_property_overrides_the_assignment_head_to_2741() {
    // reportUnmatchedProperty: the missing-property message IS
    // the head (+ related 2728 on the declaration) — oracle
    // heads.ts probe.
    assert_eq!(
        checked_rows(
            "interface A0 { x: number }\ninterface C0 { x: number; y: string }\ndeclare let a0: A0;\ndeclare let c0: C0;\nc0 = a0;\n"
        ),
        [(2741, 105, 2)]
    );
}

#[test]
fn multiple_missing_properties_override_the_head_to_2739() {
    assert_eq!(
        checked_rows(
            "interface A0 { x: number }\ninterface B0 { x: number; y: string; z: boolean }\ndeclare let a0: A0;\ndeclare let b0: B0;\nb0 = a0;\n"
        ),
        [(2739, 117, 2)]
    );
}

#[test]
fn failed_assignment_from_narrowable_union_rhs_reports_2322() {
    // Un-gated at 6.6f: the RHS position consumes the real flow
    // type (unnarrowed here — oracle-exact row).
    assert_eq!(
        checked_rows(
            "interface A0 { x: number }\ndeclare let a0: A0;\ndeclare let u0: A0 | null;\na0 = u0;\n"
        ),
        [(2322, 74, 2)]
    );
}

#[test]
fn conditional_branches_widen_under_the_assignment_context() {
    // Branch literals widen against the contextual string/number;
    // the mismatching pair reports plain 'number' (oracle ops4).
    assert_eq!(
        checked_rows(
            "declare let cs4: string;\ndeclare const c4: boolean;\ncs4 = c4 ? \"a\" : \"b\";\ncs4 = c4 ? 1 : 2;\n"
        ),
        [(2322, 74, 3)]
    );
}

// ---- m4-review S4/S5 pins (oracle: vendored tsc 6.0.3, noLib,
// strict defaults, 2026-07-19) ----

#[test]
fn this_property_condition_used_in_body_suppresses_2774() {
    // S4: tsc clean — getSymbolAtLocation(this) answers the
    // this-type's symbol on BOTH chain sides, so the body use
    // matches. Pre-fix the walker answered None for ThisKeyword
    // → 2774 @35.
    assert_eq!(
        checked_rows("class C { f() {} m() { if (this.f) { this.f(); } } }\n"),
        []
    );
}

#[test]
fn walked_or_left_reuses_cond_type() {
    // S5: tsc clean — the walked `||` LEFT reuses condType
    // (83657 has no other condition), so E.Zero is never re-typed
    // as the enum literal. Pre-fix re-checking produced 2845 @55.
    assert_eq!(
        checked_rows(
            "enum E { Zero = 0 }\ndeclare const b: number;\nconst r = E.Zero || b ? 1 : 2;\n"
        ),
        []
    );
}

#[test]
fn or_left_function_condition_reports_both_2774() {
    // S5 FN flavor: tsc 2774 @81 (right g, checked directly) AND
    // @76 (left f via condType — the union's own call-signature
    // read is empty, so only the condType route reports it).
    let mut rows = checked_rows(
        "declare const f: (() => void) | undefined;\ndeclare const g: () => void;\nif (f || g) {}\n"
    );
    rows.sort_unstable_by_key(|&(_, start, _)| start);
    assert_eq!(rows, [(2774, 76, 1), (2774, 81, 1)]);
}
