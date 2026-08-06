use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions, InferenceFlags, InferencePriority, SymbolFlags};

use crate::state::test_support::{with_program_state, with_program_state_allow_parse_diagnostics};
use crate::state::CheckerState;
use crate::structural::SignatureKind;

// ---- inferTypeArguments direct pins (M6 7.4a; production wiring
// arrives with the 7.4b chooseOverload swap) ----

/// Dig the fixture's single generic call + declaration signature
/// out and build a fresh inference context over its type
/// parameters (the 76809-76812 chooseOverload preamble, minus the
/// JS AnyDefault arm — fixtures here are .ts).
fn generic_call_setup(
    state: &mut CheckerState,
) -> (
    tsc_syntax::NodeId,
    crate::state::SignatureId,
    Vec<super::EffectiveArg>,
    crate::inference::InferenceContextId,
) {
    let (call, decl) = {
        let source = state.binder.source(0);
        let call = source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == SyntaxKind::CallExpression)
            .expect("call node");
        let decl = source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == SyntaxKind::FunctionDeclaration)
            .expect("function declaration");
        (call, decl)
    };
    let signature = state
        .get_signature_from_declaration(decl)
        .expect("signature");
    let args = state.get_effective_call_arguments(call).expect("args");
    let type_parameters = state
        .signature_of(signature)
        .type_parameters
        .clone()
        .expect("generic fixture");
    let ctx = state.create_inference_context(
        &type_parameters,
        Some(signature),
        InferenceFlags::NONE,
        None,
    );
    (call, signature, args, ctx)
}

#[test]
fn infer_type_arguments_two_pass_contextual_return() {
    with_program_state(
        &[(
            "a.ts",
            "declare function f<T>(): T;\nvar v: number = f();\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let (call, signature, args, ctx) = generic_call_setup(state);
            let inferred = state
                .infer_type_arguments(call, signature, &args, CheckMode::NORMAL, ctx)
                .expect("inference completes");
            let number = state.tables.intrinsics.number;
            assert_eq!(inferred, vec![number]);
            // (a1) 75950-75955: the ReturnType-priority pass lands
            // its candidate in the MAIN context.
            let slot_id = state.inference_context(ctx).inferences[0];
            let info = state.inference_info(slot_id);
            assert_eq!(info.candidates.as_deref(), Some(&[number][..]));
            assert_eq!(info.priority, Some(InferencePriority::RETURN_TYPE));
            // (a2) 75957-75960: returnMapper derives from the
            // FRESH priority-None returnContext's inferred part —
            // applying it resolves T.
            let return_mapper = state
                .inference_context(ctx)
                .return_mapper
                .expect("returnMapper set");
            let t = state
                .signature_of(signature)
                .type_parameters
                .clone()
                .unwrap()[0];
            let mapped = state
                .instantiate_type(t, Some(return_mapper))
                .expect("instantiate under returnMapper");
            assert_eq!(mapped, number);
        },
    );
}

#[test]
fn infer_type_arguments_implied_arity_write() {
    with_program_state(
            &[(
                "a.ts",
                "declare function h<T extends unknown[], U>(x: string, ...rest: T): U;\nvar w: number = h('a', 1, 2);\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let (call, signature, args, ctx) = generic_call_setup(state);
                state
                    .infer_type_arguments(call, signature, &args, CheckMode::NORMAL, ctx)
                    .expect("inference completes");
                // 75967-75970: type-parameter rest, no spread at or
                // after argCount (1) → impliedArity = args(3) - 1.
                let slot_id = state.inference_context(ctx).inferences[0];
                assert_eq!(state.inference_info(slot_id).implied_arity, Some(2));
            },
        );
}

#[test]
fn infer_type_arguments_implied_arity_voided_by_spread() {
    // NB `...[1, 2]` would NOT pin this: getEffectiveCallArguments
    // expands array-literal spreads into per-element synthetics
    // (76300-76338), so no spread survives to 75969 and tsc itself
    // records an arity. A non-tuple array VARIABLE spread is the
    // shape that stays a SpreadElement.
    with_program_state(
            &[(
                "a.ts",
                "declare function h<T extends unknown[], U>(x: string, ...rest: T): U;\ndeclare const arr: number[];\nvar w: number = h('a', ...arr);\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let (call, signature, args, ctx) = generic_call_setup(state);
                let slot_id = state.inference_context(ctx).inferences[0];
                state.inference_info_mut(slot_id).implied_arity = Some(99);
                state
                    .infer_type_arguments(call, signature, &args, CheckMode::NORMAL, ctx)
                    .expect("inference completes");
                // 75969: a spread at/after argCount is the EXPLICIT
                // void-0 write, not a skipped one.
                assert_eq!(state.inference_info(slot_id).implied_arity, None);
            },
        );
}

#[test]
fn infer_type_arguments_binding_pattern_skips_first_pass() {
    with_program_state(
        &[("a.ts", "declare function f<T>(): T;\nvar [a] = f();\n")],
        &CompilerOptions::default(),
        |state| {
            let (call, signature, args, ctx) = generic_call_setup(state);
            state
                .infer_type_arguments(call, signature, &args, CheckMode::NORMAL, ctx)
                .expect("inference completes");
            // 75949-75950: a pattern-derived contextual type skips
            // the a1 ReturnType-priority pass...
            let slot_id = state.inference_context(ctx).inferences[0];
            let info = state.inference_info(slot_id);
            assert!(
                info.candidates.is_none() && info.contra_candidates.is_none(),
                "a1 must not run for binding-pattern contextual types"
            );
            // ...while the a2 returnContext still derives the
            // returnMapper from the pattern type.
            assert!(state.inference_context(ctx).return_mapper.is_some());
        },
    );
}

/// Driver-level fixture check (operators.rs idiom): oracle-pinned
/// rows (tsc 6.0.3, noLib, options {} unless stated) — scratchpad
/// pins/{p,q,r}*.ts probes, 2026-07-13.
// ---- assertion-position checks (6.6 review D1; oracle-pinned
// vs vendored tsc 6.0.3 noLib, 2026-07-19) ----

#[test]
fn assertion_position_checks_report_2775_and_2776() {
    // 77639-77646: a void-returning predicate call statement over
    // a non-annotated dotted name (2775) / a non-dotted target
    // (2776).
    assert_eq!(
            checked_rows(
                "function assert(_alue: unknown): asserts _alue {}\nconst helpers = { assert };\nfunction g(x: unknown) {\n    helpers.assert(typeof x === \"string\");\n    (0, assert)(typeof x === \"string\");\n}\n"
            ),
            [(2775, 107, 14), (2695, 151, 1), (2776, 150, 11)]
        );
}

#[test]
fn assertion_position_2775_keeps_explicit_annotation_related_info() {
    let related = with_program_state(
        &[(
            "a.ts",
            "function direct(value: unknown) {\n\
                     const assert = (condition: unknown): asserts condition => {};\n\
                     assert(value);\n\
                 }\n\
                 class Test {\n\
                     assert(value: unknown): asserts value {}\n\
                 }\n\
                 function inferredReceiver(value: unknown) {\n\
                     const t1 = new Test();\n\
                     t1.assert(value);\n\
                 }\n\
                 class PropertyOwner {\n\
                     assert = (condition: unknown): asserts condition => {};\n\
                     run(value: unknown) { this.assert(value); }\n\
                 }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2775)
                .map(|diagnostic| {
                    diagnostic
                        .related
                        .iter()
                        .map(|info| (info.message.code, info.message.text.clone()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(
        related,
        [
            vec![(
                2782,
                "'assert' needs an explicit type annotation.".to_owned()
            )],
            vec![(2782, "'t1' needs an explicit type annotation.".to_owned())],
            vec![(
                2782,
                "'assert' needs an explicit type annotation.".to_owned()
            )],
        ]
    );
}

#[test]
fn private_dotted_assertion_target_resolves_clean() {
    // getTypeOfDottedName's private arm recovers the binder's
    // mangled key from the class's OWN members table — the
    // numeric-reconstruction id-space bug was the 6.6-review
    // privateNamesAssertion 2775 FP face.
    assert_eq!(
            checked_rows(
                "class Foo {\n    #p1: (v: any) => asserts v is string = (_v) => {};\n    m1(v: unknown) {\n        this.#p1(v);\n        v;\n    }\n}\nclass Foo2 {\n    #p2(_v: any): asserts _v is string {}\n    m1(v: unknown) {\n        this.#p2(v);\n        v;\n    }\n}\n"
            ),
            []
        );
}

#[test]
fn static_private_dotted_assertion_target_resolves_clean() {
    // M5 post-close review E2: STATIC privates land in the
    // class's exports table (the binder mangles both flavors
    // identically), so the mangled-key recovery searches members
    // AND exports. Oracle: tsc 6.0.3 clean
    // (verify/e2_static_private_assert.ts, 2026-07-19) — pre-fix
    // this shape reported 2775 + a downstream 2322.
    assert_eq!(
            checked_rows(
                "class S {\n    static #check(_v: unknown): asserts _v is string {}\n    static m(v: unknown) {\n        S.#check(v);\n        const s: string = v;\n        s;\n    }\n}\n"
            ),
            []
        );
}

// m6 7.6/M8: synthetic-spread iteration walk + generic-rest
// narrowing probes (tsc-probed rows, vendored 6.0.3 noLib,
// scratchpad p11/p13).

#[test]
fn variadic_synthetic_spread_is_array_like_through_its_constraint() {
    // [...T, number] expands to a VARIADIC synthetic with ty = T —
    // and isArrayLikeType(T) is TRUE in tsc too (isTypeAssignableTo
    // vs readonly any[], through the grammar-enforced array
    // constraint), so the non-array-like synthetic-spread walk is
    // unconstructible from well-formed variadics (m6-close row-5
    // evidence). The [] here is a recorded FN: the oracle row
    // (2345, 106, 7) — 'T[number]' arg vs number — rides the
    // 2xxx-band arg-check FN, not the spread walk.
    assert_eq!(
            checked_rows(
                "declare function f(...xs: number[]): void;\nfunction g<T extends unknown[]>(...args: [...T, number]) {\n  f(...args);\n}\n",
            ),
            []
        );
}

#[test]
fn non_array_synthetic_spread_reports_at_its_utf16_range_with_await_related() {
    let text = "interface Array<T> { length: number; [n: number]: T; }\n\
                    interface Promise<T> {}\n\
                    declare const p: Promise<number>;\n\
                    const 名 = 0;\n\
                    f(  ...p);\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let (call, spread) = {
            let source = state.binder.source(0);
            let call = source
                .arena
                .node_ids()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::CallExpression)
                .expect("call expression");
            let spread = source
                .arena
                .node_ids()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::SpreadElement)
                .expect("spread element");
            (call, spread)
        };
        let (pos, end) = {
            let raw = state.binder.source(0).arena.node(spread);
            (raw.pos, raw.end)
        };
        let p = state
            .resolve_file_scope_name("p", SymbolFlags::VARIABLE)
            .expect("p resolves");
        let promise = state.get_type_of_symbol(p).expect("promise type");
        let args = [super::EffectiveArg::Synthetic {
            pos,
            end,
            ty: promise,
            is_spread: true,
            tuple_name_source: None,
        }];
        let any = state.tables.intrinsics.any;
        state
            .get_spread_argument_type(
                call,
                &args,
                0,
                1,
                any,
                /*inference_context*/ None,
                CheckMode::NORMAL,
            )
            .expect("synthetic iteration reports and returns any[]");

        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2461)
            .expect("non-array synthetic spread diagnostic");
        let start_byte = text.find("...p").expect("spread text");
        let expected_start = text[..start_byte].encode_utf16().count() as u32;
        let expected_length = "...p".encode_utf16().count() as u32;
        assert_eq!(
            (diagnostic.start, diagnostic.length),
            (Some(expected_start), Some(expected_length))
        );
        assert_eq!(
            diagnostic.message.text,
            "Type 'Promise<number>' is not an array type."
        );
        assert_eq!(diagnostic.related.len(), 1);
        let related = &diagnostic.related[0];
        assert_eq!(related.message.code, 2773);
        assert_eq!(
            (related.start, related.length),
            (Some(expected_start), Some(expected_length))
        );
    });
}

#[test]
fn synthetic_spread_reports_end_to_end_from_effective_arguments() {
    let text = "const 名 = 0;\n\
                    declare function f(...xs: number[]): void;\n\
                    function g<T>(...args: [...T]) { f(  ...args); }\n";
    let (rows, partials) =
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let rows = state
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2461 | 2574))
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.expect("file diagnostic start"),
                        diagnostic.length.expect("file diagnostic length"),
                        diagnostic.message.text.clone(),
                    )
                })
                .collect::<Vec<_>>();
            (rows, state.partial_check_records.clone())
        });
    assert_eq!(
        rows,
        [
            (
                2574,
                80,
                4,
                "A rest element type must be an array type.".to_owned(),
            ),
            (2461, 93, 7, "Type 'T' is not an array type.".to_owned(),),
        ]
    );
    assert!(partials.is_empty(), "{partials:?}");
}

#[test]
fn constrained_synthetic_spread_keeps_the_array_like_non_firing_path() {
    let text = "declare function f(...xs: unknown[]): void;\n\
                    function g<T extends unknown[]>(...args: [...T]) { f(...args); }\n";
    let (codes, partials) =
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            (
                state
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.code() != 2318)
                    .map(|diagnostic| diagnostic.code())
                    .collect::<Vec<_>>(),
                state.partial_check_records.clone(),
            )
        });
    assert!(codes.is_empty(), "{codes:?}");
    assert!(partials.is_empty(), "{partials:?}");
}

#[test]
fn rejected_overload_keeps_reached_synthetic_spread_iteration_diagnostic() {
    // tsc has no candidate transaction: the first overload reaches
    // getSpreadArgumentType and emits 2461 before its tuple-union
    // relation fails. The later fixed overload succeeds, but that
    // already-emitted iteration row remains in the program sink.
    let text = "declare function f(...xs: [] | [x: string]): \"rest\";\n\
                    declare function f(x?: unknown): \"fixed\";\n\
                    function g<T>(...args: [...T]) { const r = f(...args); const fixed: \"fixed\" = r; return fixed; }\n";
    let (rows, partials) =
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let rows = state
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2322 | 2461 | 2574))
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.expect("file diagnostic start"),
                        diagnostic.length.expect("file diagnostic length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            (rows, state.partial_check_records.clone())
        });
    assert_eq!(
        rows,
        [
            (
                2574,
                119,
                4,
                "A rest element type must be an array type.".to_owned(),
            ),
            (2461, 140, 7, "Type 'T' is not an array type.".to_owned(),),
        ]
    );
    assert!(partials.is_empty(), "{partials:?}");
}

#[test]
fn earlier_fixed_overload_does_not_precheck_later_synthetic_spread_iteration() {
    // Order-sensitive non-firing twin: the fixed overload succeeds
    // before the tuple-union rest candidate is visited, so tsc
    // never calls getSpreadArgumentType and never emits 2461.
    let text = "declare function f(x?: unknown): \"fixed\";\n\
                    declare function f(...xs: [] | [x: string]): \"rest\";\n\
                    function g<T>(...args: [...T]) { const r = f(...args); const fixed: \"fixed\" = r; return fixed; }\n";
    let (rows, partials) =
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let rows = state
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2322 | 2461 | 2574))
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.expect("file diagnostic start"),
                        diagnostic.length.expect("file diagnostic length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            (rows, state.partial_check_records.clone())
        });
    assert_eq!(
        rows,
        [(
            2574,
            119,
            4,
            "A rest element type must be an array type.".to_owned(),
        )]
    );
    assert!(partials.is_empty(), "{partials:?}");
}

#[test]
fn generic_rest_narrowing_matches_oracle_with_type_variable_residue() {
    // Dependent-parameter narrowing over a generic rest type whose
    // CONSTRAINT still carries a type variable (B): the
    // non-fixing-mapper chain narrows and the access reports 2571
    // exactly like tsc — getReducedApparentType proceeds directly
    // into the union-of-tuples gate even with B still present.
    assert_eq!(
            checked_rows(
                "declare function invoke<B, A extends [\"a\", B] | [\"b\", string]>(cb: (...args: A) => void): void;\ninvoke((...args) => { if (args[0] === \"a\") { args[1].bad; } });\n",
            ),
            [(2571, 141, 7)]
        );
}

#[test]
fn generic_rest_missing_property_keeps_declaration_related_info() {
    let text = "interface Array<T> { length: number; [index: number]: T; }\n\
                    interface RequiredArray<T> extends Array<T> { required: number; }\n\
                    declare function take<T>(...args: RequiredArray<T>): void;\n\
                    take();\n";
    let (codes, related) =
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2345)
                .expect("generic rest mismatch");
            fn flatten(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
                codes.push(chain.code);
                for child in &chain.next {
                    flatten(child, codes);
                }
            }
            let mut codes = Vec::new();
            flatten(&diagnostic.message, &mut codes);
            let related = diagnostic
                .related
                .iter()
                .map(|info| (info.message.code, info.message.text.clone()))
                .collect::<Vec<_>>();
            (codes, related)
        });
    assert_eq!(codes, [2345, 2741]);
    assert_eq!(related, [(2728, "'required' is declared here.".to_owned())]);
}

#[test]
fn homomorphic_mapped_call_infers_the_source_object_shape() {
    with_program_state(
        &[(
            "a.ts",
            "declare function restore<T>(value: { [K in keyof T]: T[K] }): T;\n\
                 const value = restore({ a: \"x\", b: 1 });\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let value = state
                .resolve_file_scope_name("value", SymbolFlags::VARIABLE)
                .expect("value resolves");
            let inferred = state.get_type_of_symbol(value).expect("call result types");
            let a = state
                .get_property_of_type_full(inferred, "a")
                .expect("inferred members resolve")
                .expect("a is inferred");
            let b = state
                .get_property_of_type_full(inferred, "b")
                .expect("inferred members stay resolved")
                .expect("b is inferred");
            assert_eq!(
                state.get_type_of_symbol(a).expect("a types"),
                state.tables.intrinsics.string
            );
            assert_eq!(
                state.get_type_of_symbol(b).expect("b types"),
                state.tables.intrinsics.number
            );
        },
    );
}

#[test]
fn conditional_homomorphic_mapped_call_keeps_fresh_parameter_mapper() {
    assert_eq!(
        checked_rows(
            "type Keys<T> = keyof T extends never ? never : { [K in keyof T]: T[K] };\n\
                 declare function take<T>(value: Keys<T>): T;\n\
                 const value = take({ a: \"x\" });\n",
        ),
        []
    );
}

#[test]
fn recursive_conditional_inference_uses_root_recursion_identity() {
    assert_eq!(
            checked_rows(
                "type GetPath<T, P> = P extends readonly [] ? T : P extends readonly [infer A extends keyof T, ...infer Rest] ? GetPath<T[A], Rest> : never;\n\
                 declare function set<T, const P extends readonly string[]>(obj: T, path: P, value: GetPath<T, P>): void;\n\
                 declare const obj: { a: { b: { c: \"ok\" } } };\n\
                 declare const value: \"ok\";\n\
                 set(obj, [\"a\", \"b\", \"c\"], value);\n",
            ),
            []
        );
}

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
        strict: Some(false),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        rows(state)
    })
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

fn checked_chain(text: &str, code: u32) -> (Vec<u32>, Vec<String>) {
    checked_chain_with(text, &CompilerOptions::default(), code)
}

fn checked_chain_with(text: &str, options: &CompilerOptions, code: u32) -> (Vec<u32>, Vec<String>) {
    fn flatten(
        chain: &tsc_diagnostics::MessageChain,
        codes: &mut Vec<u32>,
        texts: &mut Vec<String>,
    ) {
        codes.push(chain.code);
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, codes, texts);
        }
    }

    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == code)
            .unwrap_or_else(|| panic!("diagnostic {code} is reported"));
        let mut codes = Vec::new();
        let mut texts = Vec::new();
        flatten(&diagnostic.message, &mut codes, &mut texts);
        (codes, texts)
    })
}

#[test]
fn tuple_spread_synthetic_report_uses_the_undefined_stripped_target() {
    let messages = with_program_state(
        &[(
            "a.ts",
            "declare function take(first?: number, second?: number): void;\n\
                 declare const tuple: [number, string];\n\
                 take(...tuple);\n",
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2345)
                .map(|diagnostic| diagnostic.message_text().to_owned())
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(
        messages,
        ["Argument of type 'string' is not assignable to parameter of type 'number'."]
    );
}

#[test]
fn tuple_spread_fixed_parameter_await_related_uses_the_synthetic_span() {
    let text = "interface Promise<T> {}\n\
                    declare function take(value: string): void;\n\
                    declare const tuple: [Promise<string>];\n\
                    const 名 = 0;\n\
                    take(  ...tuple);\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2345)
            .expect("tuple-spread fixed-parameter mismatch");
        let start_byte = text.find("...tuple").expect("spread text");
        let expected_start = text[..start_byte].encode_utf16().count() as u32;
        let expected_length = "...tuple".encode_utf16().count() as u32;
        assert_eq!(
            (diagnostic.start, diagnostic.length),
            (Some(expected_start), Some(expected_length))
        );
        assert_eq!(diagnostic.related.len(), 1);
        let related = &diagnostic.related[0];
        assert_eq!(related.message.code, 2773);
        assert_eq!(
            (related.start, related.length),
            (Some(expected_start), Some(expected_length))
        );
    });
}

#[test]
fn checked_js_missing_typed_arguments_reach_internal_arity_diagnostics() {
    let files = [
        (
            "defs.d.ts",
            "declare function f1(p: void): void;\n\
                 declare function f2(p: undefined): void;\n\
                 declare function f3(p: unknown): void;\n\
                 declare function f4(p: any): void;\n\
                 interface I<T> { m(p: T): void; }\n\
                 declare const o1: I<void>;\n\
                 declare const o2: I<undefined>;\n\
                 declare const o3: I<unknown>;\n\
                 declare const o4: I<any>;\n",
        ),
        (
            "jsfile.js",
            "f1();\no1.m();\nf2();\nf3();\nf4();\no2.m();\no3.m();\no4.m();\n",
        ),
    ];
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let actual = with_program_state(&files, &options, |state| {
        state.check_source_file(1);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2554)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone(),
                    diagnostic.start,
                    diagnostic.length,
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(actual.len(), 6);
    assert!(actual
        .iter()
        .all(|(file_name, _, _)| { file_name.as_deref() == Some("jsfile.js") }));

    let non_strict_options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let non_strict_count = with_program_state(&files, &non_strict_options, |state| {
        state.check_source_file(1);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2554)
            .count()
    });
    assert_eq!(non_strict_count, 0);
}

#[test]
fn unnamed_jsdoc_parameter_arity_related_uses_argument_index() {
    let related = with_program_state(
        &[(
            "a.js",
            "/** @type {function(string): void} */\n\
                 const f = (value) => {};\n\
                 /** @type {(s: string) => void} */\n\
                 function g(s) {}\n\
                 f();\n\
                 g();\n",
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2554)
                .map(|diagnostic| {
                    let related = diagnostic
                        .related
                        .first()
                        .expect("arity diagnostics carry parameter provenance");
                    (related.message.code, related.message.text.clone())
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(
        related,
        [
            (6210, "An argument for '0' was not provided.".to_owned()),
            (6210, "An argument for 's' was not provided.".to_owned()),
        ]
    );
}

#[test]
fn production_overload_boundary_rolls_back_then_commits() {
    with_program_state(
        &[(
            "a.ts",
            "declare function pick(x: string): \"s\";\n\
                 declare function pick(x: number): \"n\";\n\
                 const selected = pick(1);\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let commits_before = state.speculation_commit_count;
            let rollbacks_before = state.speculation_rollback_count;
            state.check_source_file(0);
            assert!(rows(state).is_empty());
            assert!(
                state.speculation_rollback_count > rollbacks_before,
                "the rejected string candidate must roll back"
            );
            assert!(
                state.speculation_commit_count > commits_before,
                "the selected number candidate must commit"
            );
            assert_eq!(state.speculation_depth, 0);
        },
    );
}

#[test]
fn selected_generic_object_candidate_keeps_contextual_method_returns() {
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
            checked_rows_with(
                "interface Component { data: any; el: any; init(): void; update(): void; }\n\
                 type PartialLike<X> = { [P in keyof X]?: X[P] };\n\
                 interface ThisType<T> {}\n\
                 declare function registerComponent<T extends object>(component: T & PartialLike<Component> & ThisType<T & Component>): T;\n\
                 const value = registerComponent({\n\
                   init() { this.data.n = 0; this.el.use(); },\n\
                   update() {},\n\
                   extra() { return this.data.n; }\n\
                 });\n",
                &options,
            ),
            [(6133, 141, 3)]
        );
}

#[test]
fn failed_candidate_keeps_deferred_assertion_operand_stash() {
    with_program_state(
        &[(
            "a.ts",
            "declare function use(x: string): void;\n\
                 declare function use(x: number): void;\n\
                 use(1 as number);\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let assertion = state
                .binder
                .source(0)
                .arena
                .node_ids()
                .find(|&node| state.kind_of(node) == SyntaxKind::AsExpression)
                .expect("assertion node");
            state.check_source_file(0);
            assert!(rows(state).is_empty());
            assert!(state
                .links
                .node(assertion)
                .assertion_expression_type
                .is_some());
            assert_eq!(state.speculation_depth, 0);
        },
    );
}

// ---- M6-stub observability (risk #1) ----

#[test]
fn generic_call_without_typeargs_contains() {
    // Oracle: clean — the stub result (unknown where tsc infers
    // number) would poison downstream types, so the statement
    // contains (honest FN of nothing here).
    assert_eq!(
        checked_rows("declare function identity<T>(x: T): T;\nidentity(1);\n"),
        []
    );
}

#[test]
fn generic_callback_contravariance_contains_not_2345() {
    // Oracle: 6133-only (M7 unused). A stub-instantiated
    // applicability verdict would fabricate a 2345 tsc never
    // reports — the whole resolution escapes instead.
    assert_eq!(
        checked_rows("declare function g<T>(f: (x: T) => void): void;\ng((x: number) => {});\n"),
        [(6133, 51, 1)]
    );
}

#[test]
fn explicit_typearg_argument_mismatch_reports_2345() {
    assert_eq!(
        checked_rows("declare function f<T>(x: T): T;\nf<number>(\"x\");\n"),
        [(2345, 42, 3)]
    );
}

#[test]
fn generic_arity_error_stays_live_under_the_stub() {
    // Arity verdicts read declared parameter counts — stub-free.
    assert_eq!(
        checked_rows("declare function f<T>(a: T, b: T): void;\nf(1);\n"),
        [(2554, 41, 1)]
    );
}

// ---- deferred re-check (risk #2) ----

#[test]
fn arity_failed_call_rechecks_args_with_candidate_types() {
    // The stashed failure candidate types `x` as string; the
    // deferred plain re-check then reports the noLib 2339 on
    // `length` — NOT a 7006 implicit-any (oracle-pinned pair).
    assert_eq!(
        checked_rows(
            "declare function f(cb: (x: string) => void, b: number): void;\nf((x) => x.length);\n"
        ),
        [(2554, 62, 1), (2339, 73, 6)]
    );
}

// ---- code identity pairs (risk #3) ----

#[test]
fn construct_only_target_reports_2348_with_display() {
    assert_eq!(
        checked_rows("interface Ctor { new (x: number): object }\ndeclare const c: Ctor;\nc(1);\n"),
        [(2348, 66, 4)]
    );
    // The anonymous-typed flavor renders the construct shorthand
    // (9.3b2 signature rung; oracle-probed: 2348 displaying
    // 'new (x: number) => object').
    assert_eq!(
        checked_rows("declare const c: { new (x: number): object };\nc(1);\n"),
        [(2348, 46, 4)]
    );
}

#[test]
fn rest_under_min_reports_2555_not_2554() {
    assert_eq!(
        checked_rows("declare function r(a: number, ...rest: string[]): void;\nr();\n"),
        [(2555, 56, 1)]
    );
}

#[test]
fn between_overload_bounds_reports_2575() {
    assert_eq!(
            checked_rows(
                "declare function m(a: number): void;\ndeclare function m(a: number, b: string, c: boolean): void;\nm(1, \"x\");\n"
            ),
            [(2575, 97, 1)]
        );
}

#[test]
fn single_signature_typearg_arity_reports_2558_on_the_range() {
    assert_eq!(
        checked_rows("declare function t<T, U>(x: T): void;\nt<number>(1);\n"),
        [(2558, 40, 6), (6133, 22, 1)]
    );
}

#[test]
fn overload_typearg_brackets_report_2743() {
    assert_eq!(
            checked_rows(
                "declare function ta<T>(x: T): void;\ndeclare function ta<T, U, V>(x: T): void;\nta<string, number>(\"a\");\n"
            ),
            [(2743, 81, 14), (6133, 59, 1), (6133, 62, 1)]
        );
}

#[test]
fn boundary_only_multi_signature_typeargs_report_2558() {
    assert_eq!(
            checked_rows(
                "declare function tt<T, U>(x: T, y: U): void;\ndeclare function tt<T>(x: T): void;\ntt<string, number, boolean>(\"a\", 1);\n"
            ),
            [(2558, 84, 23)]
        );
}

#[test]
fn new_over_call_signatures_reports_7009_under_strict_default() {
    assert_eq!(
            checked_rows(
                "declare function nvv(): void;\ndeclare function nvo(): number;\nnew nvv();\nnew nvo();\n"
            ),
            [(7009, 62, 9), (7009, 73, 9)]
        );
}

#[test]
fn no_implicit_any_off_swaps_7009_for_the_2350_band() {
    // Void-returning new-over-call is CLEAN; non-void reports
    // 2350; 7009 is gone (oracle-pinned option flip).
    let options = CompilerOptions {
        no_implicit_any: Some(false),
        ..CompilerOptions::default()
    };
    assert_eq!(
            checked_rows_with(
                "declare function nvv(): void;\ndeclare function nvo(): number;\nnew nvv();\nnew nvo();\n",
                &options
            ),
            [(2350, 73, 9)]
        );
}

#[test]
fn untyped_call_with_typeargs_reports_2347_at_the_call() {
    assert_eq!(
        checked_rows("declare const anyv: any;\nanyv<number>(1);\n"),
        [(2347, 25, 15)]
    );
}

// ---- span discipline (risk #4) ----

#[test]
fn plain_argument_mismatch_reports_2345_at_the_arg() {
    assert_eq!(
        checked_rows("declare function s(a: number): void;\ns(\"x\");\n"),
        [(2345, 39, 3)]
    );
}

#[test]
fn under_arity_reports_at_the_callee_name_span() {
    assert_eq!(
        checked_rows("declare const obj: { m(a: number): void };\nobj.m();\n"),
        [(2554, 47, 1)]
    );
}

#[test]
fn over_arity_reports_at_the_excess_args_range() {
    assert_eq!(
        checked_rows("declare function v(a: number): void;\nv(1, 2, 3);\n"),
        [(2554, 42, 4)]
    );
}

#[test]
fn jsdoc_function_type_models_this_and_new_parameters() {
    assert_eq!(
            checked_rows(
                "function hof2(f: function(this: number, string): string) {\n    return f(12, 'hullo');\n}\n"
            ),
            [(8020, 17, 38), (2554, 76, 7)]
        );
    assert_eq!(
        checked_rows(
            "function hof(ctor: function(new: number, string)) {\n    return new ctor('hi');\n}\n"
        ),
        [(8020, 19, 29)]
    );
}

#[test]
fn jsdoc_class_tag_requires_new() {
    let rows = checked_js_rows(
        "/** @constructor */\nfunction Dependency(j) { return j; }\nDependency({});\n",
    );
    assert_eq!(
        rows.into_iter()
            .filter(|(code, _, _)| *code == 2348)
            .collect::<Vec<_>>(),
        [(2348, 57, 14)]
    );
}

#[test]
fn untyped_js_signature_has_zero_minimum_arity() {
    let rows = checked_js_rows("function f(required) {}\nf();\n");
    assert!(rows.into_iter().all(|(code, _, _)| code != 2554));
}

#[test]
fn overload_over_arity_uses_the_union_failure_candidate() {
    assert_eq!(
            checked_rows(
                "declare function two(a: number): void;\ndeclare function two(a: string): void;\ntwo(1, 2);\n"
            ),
            [(2554, 85, 1)]
        );
}

#[test]
fn untupled_spread_reports_2556_at_the_spread_arg() {
    assert_eq!(
            checked_rows(
                "declare function sp(a: number, b: number): void;\ndeclare const xs: number[];\nsp(...xs);\n"
            ),
            [(2556, 80, 5)]
        );
}

// ---- overload failure chains (2769 band) ----

#[test]
fn two_failed_overloads_report_2769_at_the_shared_span() {
    let text =
        "declare function o(a: number): void;\ndeclare function o(a: string): void;\no(true);\n";
    assert_eq!(checked_rows(text), [(2769, 76, 4)]);
    let (codes, texts) = checked_chain(text, 2769);
    assert_eq!(codes, [2769, 2772, 2345, 2772, 2345]);
    assert_eq!(
        texts[1],
        "Overload 1 of 2, '(a: number): void', gave the following error."
    );
    assert_eq!(
        texts[3],
        "Overload 2 of 2, '(a: string): void', gave the following error."
    );
}

#[test]
fn two_failed_overloads_preserve_present_empty_related_information() {
    let text =
        "declare function o(a: number): void;\ndeclare function o(a: string): void;\no(true);\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2769)
            .expect("the overload aggregate is reported");
        assert!(diagnostic.related_information_present);
        assert!(diagnostic.related.is_empty());
    });
}

#[test]
fn many_failed_overloads_report_2769_at_the_last_failure() {
    let text = "declare function w(a: number): void;\ndeclare function w(a: string): void;\ndeclare function w(a: boolean): void;\ndeclare function w(a: object): void;\nw(null);\n";
    assert_eq!(checked_rows(text), [(2769, 151, 4)]);
    let (codes, _) = checked_chain(text, 2769);
    assert_eq!(codes, [2769, 2770, 2345]);
}

// ---- invocation errors ----

#[test]
fn union_with_uncallable_constituent_reports_one_2349_row() {
    assert_eq!(
        checked_rows("declare const u: { (): void } | { n: number };\nu();\n"),
        [(2349, 47, 1)]
    );
}

#[test]
fn invocation_error_details_match_the_tsc_union_ladder() {
    fn chain_codes(text: &str, code: u32) -> (Vec<u32>, Vec<String>) {
        fn flatten(
            chain: &tsc_diagnostics::MessageChain,
            codes: &mut Vec<u32>,
            texts: &mut Vec<String>,
        ) {
            codes.push(chain.code);
            texts.push(chain.text.clone());
            for child in &chain.next {
                flatten(child, codes, texts);
            }
        }

        let source_text = format!(
                "interface Object {{}}\ninterface Function {{ readonly prototype: any }}\ninterface Array<T> {{}}\ninterface ReadonlyArray<T> {{}}\n{text}"
            );
        with_program_state(
            &[("a.ts", &source_text)],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let diagnostic = state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == code)
                    .unwrap_or_else(|| {
                        panic!(
                            "the invocation error {code} is reported for {text:?}; actual={:?}",
                            state
                                .diagnostics
                                .iter()
                                .map(|diagnostic| diagnostic.code())
                                .collect::<Vec<_>>()
                        )
                    });
                let mut codes = Vec::new();
                let mut texts = Vec::new();
                flatten(&diagnostic.message, &mut codes, &mut texts);
                (codes, texts)
            },
        )
    }

    let (plain_call, plain_call_text) =
        chain_codes("declare const value: { n: number };\nvalue();\n", 2349);
    assert_eq!(plain_call, [2349, 2757]);
    assert_eq!(
        plain_call_text[1],
        "Type '{ n: number; }' has no call signatures."
    );

    let (plain_construct, plain_construct_text) =
        chain_codes("declare const value: { n: number };\nnew value();\n", 2351);
    assert_eq!(plain_construct, [2351, 2761]);
    assert_eq!(
        plain_construct_text[1],
        "Type '{ n: number; }' has no construct signatures."
    );

    let (none, _) = chain_codes(
        "declare const value: { a: string } | { b: number };\nvalue();\n",
        2349,
    );
    assert_eq!(none, [2349, 2755]);

    let (mixed, _) = chain_codes(
        "declare const value: { (): void } | { n: number };\nvalue();\n",
        2349,
    );
    assert_eq!(mixed, [2349, 2756, 2757]);

    let (incompatible, _) = chain_codes(
        r#"
interface A { a: string }
interface B { b: number }
interface C { c: string }
interface D { d: number }
interface F3 {
    (this: A): void;
    (this: B): void;
}
interface F4 {
    (this: C): void;
    (this: D): void;
}
declare const value: F3 | F4;
value();
"#,
        2349,
    );
    assert_eq!(incompatible, [2349, 2758]);

    let (never_intersection, never_intersection_text) = chain_codes(
        "declare const value: { (x: string): number; a: \"\" } & { a: number };\nvalue();\n",
        2349,
    );
    assert_eq!(never_intersection, [2349, 2757]);
    assert_eq!(
        never_intersection_text[1],
        "Type 'never' has no call signatures."
    );

    assert_eq!(
        checked_rows("declare const callable: () => void;\ncallable();\n"),
        []
    );
}

// ---- the Invoke non-null reporter ----

#[test]
fn nullable_narrowable_callee_reports_2721() {
    // Un-gated at 6.6f (oracle-exact row).
    assert_eq!(
        checked_rows("declare const nf: (() => void) | null;\nnf();\n"),
        [(2721, 39, 2)]
    );
}

#[test]
fn nullable_unnarrowable_callee_reports_2721() {
    assert_eq!(
        checked_rows("((null as unknown) as (() => void) | null)();\n"),
        [(2721, 0, 42)]
    );
}

// ---- this arguments ----

#[test]
fn this_parameter_mismatch_reports_2684_at_the_call() {
    assert_eq!(
        checked_rows(
            "interface N { n: number; }\ndeclare function th(this: N, a: number): void;\nth(1);\n"
        ),
        [(2684, 74, 5)]
    );
}

// ---- optional-chain results (risk #9) ----

#[test]
fn outer_chain_call_result_takes_the_optional_union() {
    // The IsOuterCallChain return arm adds `undefined` — dropping
    // it would leave a plain `number` RHS and kill this 2322
    // (the assignment shape rides 5.5e; const statements are 5.8).
    assert_eq!(
            checked_rows(
                "declare const oc: { b(): number } | undefined;\ndeclare let sink: number;\nsink = oc?.b();\n"
            ),
            [(2322, 73, 4)]
        );
}

// ---- elaboration gate ----

#[test]
fn array_literal_args_against_non_array_params_contain() {
    // Oracle: plain 2345 at the literal (elaboration finds no
    // rows) — LIVE since M6 7.5's "{}" display arm un-contained
    // the head (args '{}' vs 'I'; re-probed probe75d.mjs). The
    // element contextual read runs the live §4 Element probe
    // (silently None on I).
    assert_eq!(
        checked_rows("interface I { p: string }\ndeclare function el(a: I): void;\nel([1]);\n"),
        [(2345, 62, 3)]
    );
    // Tuple targets check the elements fine; the plain head now
    // renders through the 9.3a tuple renderer. Oracle: (2345, 45,
    // 8) "Argument of type '[number, string]' is not assignable
    // to parameter of type '[number]'." (the arity chain rides
    // the elided tail).
    assert_eq!(
        checked_rows("declare function tup(a: [number]): void;\ntup([1, \"x\"]);\n"),
        [(2345, 45, 8)]
    );
}

#[test]
fn array_literal_arg_reports_elementwise_row() {
    // Oracle: 2322 at the element — the elementwise elaboration
    // replaces the 2345 argument head.
    assert_eq!(
        checked_rows("declare function tup(a: [number]): void;\ntup([\"x\"]);\n"),
        [(2322, 46, 3)]
    );
}

#[test]
fn array_member_elaboration_rechecks_the_syntax_specific_source() {
    with_program_state(
            &[(
                "a.ts",
                "function b7([[a], b, [[c, d]]] = [[undefined], undefined, [[undefined, undefined]]]) {}\n\
                 b7([[\"string\"], 1, [[true, false]]]);\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let messages = state
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.code() == 2322)
                    .map(|diagnostic| diagnostic.message_text())
                    .collect::<Vec<_>>();
                assert_eq!(
                    messages,
                    [
                        "Type 'string' is not assignable to type 'undefined'.",
                        "Type 'number' is not assignable to type 'undefined'.",
                        "Type 'true' is not assignable to type 'undefined'.",
                        "Type 'false' is not assignable to type 'undefined'.",
                    ]
                );
            },
        );
}

#[test]
fn object_literal_arg_reports_member_row() {
    // elaborateObjectLiteral → elaborateElementwise: the row
    // anchors at the property name, not the outer argument.
    assert_eq!(
        checked_rows("declare function f(a:{x:number}):void;\nf({x:\"s\"});\n"),
        [(2322, 42, 1)]
    );
}

#[test]
fn object_member_elaboration_uses_the_mutable_source_property() {
    with_program_state(
        &[(
            "a.ts",
            "declare function f(x: { a: true }): void;\n\
                 f({ a: 1 });\n\
                 f({ a: 1 as const });\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostics: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
                .collect();
            assert_eq!(diagnostics.len(), 2);
            assert_eq!(
                diagnostics[0].message_text(),
                "Type 'number' is not assignable to type 'true'."
            );
            // Non-firing sibling: an explicit const assertion
            // keeps the singleton source through the same helper.
            assert_eq!(
                diagnostics[1].message_text(),
                "Type '1' is not assignable to type 'true'."
            );
            for diagnostic in diagnostics {
                assert_eq!(diagnostic.related.len(), 1);
                assert_eq!(diagnostic.related[0].message.code, 6500);
                assert_eq!(
                    diagnostic.related[0].message.text,
                    "The expected type comes from property 'a' which is declared here on type \
                         '{ a: true; }'"
                );
            }
        },
    );
}

#[test]
fn reverse_mapped_inference_keeps_elementwise_property_origin() {
    with_program_state(
        &[(
            "a.ts",
            "type ComputedOf<T> = { [K in keyof T]: () => T[K] };\n\
                 declare function f<C>(value: { computed: ComputedOf<C> }): void;\n\
                 f({ computed: { baz: 42 } });\n",
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the reverse-mapped property mismatch is reported");
            assert_eq!(
                diagnostic.message_text(),
                "Type 'number' is not assignable to type '() => unknown'."
            );
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(diagnostic.related[0].message.code, 6500);
            assert_eq!(
                diagnostic.related[0].message.text,
                "The expected type comes from property 'baz' which is declared here on type \
                     'ComputedOf<{ baz: unknown; }>'"
            );
        },
    );
}

#[test]
fn elementwise_elaboration_points_to_an_index_signature() {
    with_program_state(
        &[(
            "a.ts",
            "declare function f(x: { [key: string]: number }): void;\nf({ a: \"x\" });\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the property mismatch is reported");
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(diagnostic.related[0].message.code, 6501);
            assert_eq!(
                diagnostic.related[0].message.text,
                "The expected type comes from this index signature."
            );
        },
    );
}

#[test]
fn ordinary_declaration_roots_retain_elementwise_provenance() {
    with_program_state(
        &[
            ("types.d.ts", "interface Target { a: number; }\n"),
            (
                "a.ts",
                "declare function f(x: Target): void;\nf({ a: \"x\" });\n",
            ),
        ],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(1);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the property mismatch is reported");
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(diagnostic.related[0].message.code, 6500);
            assert_eq!(
                diagnostic.related[0].file_name.as_deref(),
                Some("types.d.ts")
            );
        },
    );
}

#[test]
fn arrow_elaboration_points_to_the_contextual_return_signature() {
    with_program_state(
        &[("a.ts", "const f: () => number = () => \"x\";\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the return mismatch is reported");
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(diagnostic.related[0].message.code, 6502);
            assert_eq!(
                diagnostic.related[0].message.text,
                "The expected type comes from the return type of this signature."
            );
        },
    );
}

#[test]
fn declined_object_elaboration_keeps_excess_property_selection() {
    // The elementwise walk skips unknown target properties, then
    // the ordinary relation reporter selects 2353 at `y`.
    assert_eq!(
        checked_rows("declare function f(a:{x:number}):void;\nf({x:1,y:2});\n"),
        [(2353, 46, 1)]
    );
}

#[test]
fn arrow_argument_elaborates_the_return_expression() {
    assert_eq!(
        checked_rows("declare function f(a:()=>number):void;\nf(()=>\"s\");\n"),
        [(2322, 45, 3)]
    );
}

#[test]
fn spread_array_argument_reports_the_tupleized_element() {
    // elaborateArrayLiteral force-tuples the source; the spread
    // syntax position then indexes the tupleized string element.
    assert_eq!(
            checked_rows(
                "declare function f(a:[number,number]):void;\nconst xs:[string]=[\"s\"];\nf([1,...xs]);\n"
            ),
            [(2322, 74, 5)]
        );
}

#[test]
fn overload_probe_uses_the_elaborated_member_span() {
    // errorOutputContainer.skipLogging captures both overload
    // tails; the shared inner `x` span anchors the outer 2769.
    assert_eq!(
            checked_rows(
                "declare function f(a:{x:number}):void;\ndeclare function f(a:{x:boolean}):void;\nf({x:\"s\"});\n"
            ),
            [(2769, 82, 1)]
        );
}

#[test]
fn function_valued_argument_success_is_clean() {
    assert_eq!(
            checked_rows(
                "declare function cb(f: () => number): void;\ndeclare function mk(): number;\ncb(mk);\n"
            ),
            []
        );
}

#[test]
fn optional_method_chain_call_reports_2722() {
    // Un-gated at 6.6f (oracle-exact row).
    assert_eq!(
        checked_rows("declare const om: { m?(): void };\nom.m();\n"),
        [(2722, 34, 4)]
    );
}

// ---- createNormalizedTupleType checker twin (5.3 residuals) ----

#[test]
fn tuple_constraint_this_append_resolves() {
    // checkTypeArguments' constraint check runs
    // getTypeWithThisArgument over the TUPLE constraint (the
    // this slot appends past the element list — tsc's undefined
    // element-flag read, zero flags here); the surviving arg
    // mismatch renders 2345 with intrinsic display.
    assert_eq!(
            checked_rows(
                "declare function f<T extends [number, string?]>(x: T, n: number): void;\nf<[1, \"a\"]>([1, \"a\"], \"no\");\n"
            ),
            [(2345, 94, 4)]
        );
}

#[test]
fn union_variadic_tuple_distributes_over_constituents() {
    // [...(A | B)] distributes via mapType — formerly an
    // M4Dependency containment of the whole alias. The demand
    // avoids tuple DISPLAY (still a T2 curtain): u[0] is 1 | 2.
    assert_eq!(
            checked_rows(
                "type A = [1];\ntype B = [2];\ntype U = [...(A | B)];\ndeclare const u: U;\nu[0].bad;\n"
            ),
            [(2339, 76, 3)]
        );
}

#[test]
fn array_like_variadic_element_collapses_to_rest() {
    // [...Arr, number] with Arr = string[]: the array-like arm
    // reads the number index type into a Rest element (noLib
    // degrades the element to any — matching the oracle).
    assert_eq!(
            checked_rows(
                "type Arr = string[];\ntype V = [...Arr, number];\ndeclare const v: V;\ndeclare function takeNever(x: never): void;\ntakeNever(v[0]);\n"
            ),
            [(2345, 122, 4)]
        );
}

#[test]
fn reference_source_vs_tuple_target_relates_structurally() {
    // A non-array reference source against a tuple target rides
    // the property machinery (the stale M3-era escape contained
    // the whole call); the 2-3 overload ladder renders 2769
    // with the two overload-specific diagnostic subtrees.
    assert_eq!(
            checked_rows(
                "interface Box<T> { v: T }\ndeclare const b: Box<string>;\ndeclare function f(x: [number]): void;\ndeclare function f(x: number): void;\nf(b);\n"
            ),
            [(2769, 134, 1)]
        );
}

#[test]
fn variadic_in_rest_window_collapses_via_indexed_access() {
    // [...string[], ...T]: the generic variadic sits inside the
    // rest window at declaration normalization (T[number] via
    // getIndexedAccessType); W<[boolean]> re-normalizes and w[0]
    // reads string | boolean.
    assert_eq!(
            checked_rows(
                "type W<T extends unknown[]> = [...string[], ...T];\ndeclare const w: W<[boolean]>;\ndeclare function takeNever(x: never): void;\ntakeNever(w[0]);\n"
            ),
            [(2345, 136, 4)]
        );
}

// ---- 5.7b: tagged templates (scratchpad pins/{p,q,r}*.ts,
// oracle-probed 2026-07-13) ----

#[test]
fn tagged_substitution_mismatch_reports_2345_at_the_expression() {
    assert_eq!(
            checked_rows(
                "declare function tag(s: any, x: number): void;\ndeclare var s: string;\ntag`a${s}b`;\n"
            ),
            [(2345, 77, 1)]
        );
}

#[test]
fn tagged_under_arity_reports_2554_at_the_whole_tagged_node() {
    // getDiagnosticForCallNode: non-CallExpression call-likes take
    // the NODE span (no callee-name narrowing).
    assert_eq!(
        checked_rows("declare function tag(s: any, x: number, y: number): void;\ntag`a${1}b`;\n"),
        [(2554, 58, 11)]
    );
}

#[test]
fn adjacent_templates_in_array_literal_are_untyped_under_no_lib() {
    // Oracle: CLEAN under noLib — the string-literal tag is
    // assignable to the degenerate (empty-object) Function global,
    // so isUntypedFunctionCall wins before the 2796 comma hint.
    // The 2796 face needs libs (conformance corpus covers it).
    assert_eq!(checked_rows("const a = [ `x` `y` ];\n"), []);
}

#[test]
fn optional_chain_tagged_template_reports_1358_at_the_template() {
    assert_eq!(
        checked_rows("declare const t: { m: any };\nt?.m`x`;\n"),
        [(1358, 33, 3)]
    );
}

#[test]
fn template_strings_array_arg_is_no_lib_silent() {
    // Oracle: CLEAN — TemplateStringsArray misses under noLib
    // (locationless 2318, dropped) and the emptyObjectType
    // effective arg rides the degenerate T[] relation.
    assert_eq!(
        checked_rows("declare function tag(s: string[]): void;\ntag`a`;\n"),
        []
    );
}

// ---- 5.7b: import calls ----

#[test]
fn import_call_reports_2711_under_no_lib() {
    // Expression-statement position (the demand caveat: an
    // unreferenced `const p = import(...)` initializer stays
    // unchecked until 5.8). The module band is LIVE (5.8d): the
    // unresolvable "./m" reports 2307 at the specifier; the
    // locationless Promise 2318 drops.
    assert_eq!(
        checked_rows("import(\"./m\");\n"),
        [(2307, 7, 5), (2711, 0, 13)]
    );
}

#[test]
fn import_specifier_must_be_string_7036() {
    assert_eq!(checked_rows("import(42);\n"), [(7036, 7, 2), (2711, 0, 10)]);
}

#[test]
fn import_with_no_arguments_reports_1450() {
    assert_eq!(checked_rows("import();\n"), [(1450, 0, 8), (2711, 0, 8)]);
}

#[test]
fn import_second_argument_reports_1324_at_the_default_module_kind() {
    // ES2025 target computes ModuleKind.ES2022 — outside the
    // esnext/node16+/preserve band, a second argument is 1324.
    assert_eq!(
        checked_rows("import(\"./m\", {});\n"),
        [(1324, 14, 2), (2307, 7, 5), (2711, 0, 17)]
    );
}

#[test]
fn import_defer_reports_18060_at_the_default_module_kind() {
    assert_eq!(
        checked_rows("import.defer(\"./m\");\n"),
        [(18060, 0, 19), (2307, 13, 5), (2711, 0, 19)]
    );
}

#[test]
fn import_assert_key_reports_2880_under_esnext_module() {
    let options = CompilerOptions {
        module: Some(99),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with("import(\"./m\", { assert: {} });\n", &options),
        [(2880, 16, 6), (2307, 7, 5), (2711, 0, 29)]
    );

    let silenced = CompilerOptions {
        ignore_deprecations: Some("6.0".to_owned()),
        ..options
    };
    assert_eq!(
        checked_rows_with("import(\"./m\", { assert: {} });\n", &silenced),
        [(2307, 7, 5), (2711, 0, 29)]
    );
}

#[test]
fn import_under_es2015_module_reports_1323() {
    let options = CompilerOptions {
        module: Some(5),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with("import(\"./m\");\n", &options),
        [(1323, 0, 13), (2307, 7, 5), (2711, 0, 13)]
    );
}

#[test]
fn verbatim_commonjs_import_call_reports_the_whole_call() {
    let text = "import(\"./m\");\n";
    let commonjs = CompilerOptions {
        module: Some(1),
        target: Some(99),
        module_resolution: Some(100),
        verbatim_module_syntax: Some(true),
        ..CompilerOptions::default()
    };
    let commonjs_rows = checked_rows_with(text, &commonjs)
        .into_iter()
        .filter(|row| row.0 == 1295)
        .collect::<Vec<_>>();
    assert_eq!(commonjs_rows, [(1295, 0, 13)]);

    let esm = CompilerOptions {
        module: Some(99),
        ..commonjs
    };
    assert!(
        checked_rows_with(text, &esm)
            .into_iter()
            .all(|row| row.0 != 1295),
        "ES module kind is the adjacent negative control"
    );
}

// ---- 5.7b: unique-symbol tail ----

#[test]
fn matching_unique_symbol_argument_is_clean_and_mismatch_contains() {
    // Oracle: 2345 at `w` (116+1) — FLIPPED LIVE at 9.3b5: the
    // unique-symbol display landed (plain face = the
    // AllowUniqueESSymbolType operator keyword, FQ face = the
    // typeof chain), so the pinned oracle row emits; the passing
    // `wantU(u)` row still pins the type identity.
    assert_eq!(
            checked_rows(
                "declare const u: unique symbol;\ndeclare function wantU(x: typeof u): void;\ndeclare const w: symbol;\nwantU(u);\nwantU(w);\n"
            ),
            [(2345, 116, 1)]
        );
}

#[test]
fn unique_symbol_reassignment_2322_is_a_statement_band_row() {
    // Oracle: 2322 at `v` (38+1) — FLIPPED LIVE at 9.3b5 with the
    // unique-symbol display (the 5.8 statement band was already
    // reporting; only the face was curtained).
    assert_eq!(
        checked_rows("declare const u: unique symbol;\nconst v: unique symbol = u;\nv;\n"),
        [(2322, 38, 1)]
    );
}

// ---- 5.7b: IIFE contextual parameters ----

#[test]
fn iife_argument_types_flow_into_untyped_parameters() {
    // The IIFE arm widens the literal argument: x: string, so the
    // call result feeds want() the 2345.
    assert_eq!(
            checked_rows(
                "declare function want(n: number): void;\nconst r = (function(x){ return x; })(\"s\");\nwant(r);\n"
            ),
            [(2345, 88, 1)]
        );
}

// ---- 5.7b review round 2: late-bound members in intersections ----

#[test]
fn late_bound_type_literal_does_not_collapse_in_intersections() {
    // isEmptyAnonymousObjectType must read the LATE-BOUND member
    // table: with the raw early table, O looked empty and O & "s"
    // degenerated to "s" — silencing the oracle's 2345 (probed
    // w1.ts, 2026-07-13).
    assert_eq!(
            checked_rows(
                "const k = \"kk\";\ntype O = { [k]: number };\ntype X = O & \"s\";\ndeclare const s: \"s\";\ndeclare function f(x: X): void;\nf(s);\n"
            ),
            [(2345, 116, 1)]
        );
}

// ---- 5.7b: outer type parameters across function expressions ----

#[test]
fn type_alias_inside_context_sensitive_arrow_resolves() {
    // typeof y = number through the contextual parameter; the
    // assignment face reports 2322 at `z` (the 5.5e operator
    // band). Un-escaped by the isContextSensitive replay arm.
    assert_eq!(
            checked_rows(
                "declare function take(f: (x: number) => void): void;\ntake((y) => { type L = typeof y; let z: L; z = \"s\"; void z; });\n"
            ),
            [(2322, 96, 1)]
        );
}

// ---- 5.8c §10 decorators (oracle: scratchpad probe.sh p9/p13-15,
// 2026-07-14; both modes pinned per risk #14) ----

fn legacy_decorator_options() -> CompilerOptions {
    CompilerOptions {
        experimental_decorators: true,
        ..CompilerOptions::default()
    }
}

#[test]
fn invalid_legacy_parameter_decorator_has_no_effective_arguments() {
    // getEffectiveDecoratorArguments' Debug.fail is unreachable in
    // tsc's ordinary resolveDecorator flow. Pin the deterministic
    // no-diagnostic value for a recovery decorator on a free
    // function parameter, whose legacy decorator signature is
    // intentionally absent.
    with_program_state_allow_parse_diagnostics(
        &[(
            "a.ts",
            "declare const dec: any;\nfunction f(@dec x: number) {}\n",
        )],
        &legacy_decorator_options(),
        |state| {
            let decorator = {
                let source = state.binder.source(0);
                source
                    .arena
                    .node_ids()
                    .find(|&node| source.arena.node(node).kind == SyntaxKind::Decorator)
                    .expect("decorator node")
            };
            let args = state
                .get_effective_decorator_arguments(decorator)
                .expect("invalid decorator recovers without containment");
            assert!(args.is_empty());
        },
    );
}

#[test]
fn unsupported_decorated_member_key_recovers_to_error_type() {
    // getClassElementPropertyKeyType Debug.fail's on a bigint
    // property name. The parser admits that already-invalid
    // decorated member, so C6 keeps checking with errorType rather
    // than fabricating a string/symbol key or containing the file.
    with_program_state_allow_parse_diagnostics(
        &[(
            "a.ts",
            "declare const dec: any;\nclass C { @dec 1n() {} }\n",
        )],
        &legacy_decorator_options(),
        |state| {
            let method = {
                let source = state.binder.source(0);
                source
                    .arena
                    .node_ids()
                    .find(|&node| source.arena.node(node).kind == SyntaxKind::MethodDeclaration)
                    .expect("decorated bigint method")
            };
            let key_type = state
                .get_class_element_property_key_type(method)
                .expect("unsupported property key recovers without containment");
            assert!(state.tables.is_error_type(key_type));
        },
    );
}

#[test]
fn uncalled_decorator_reports_1329_in_both_modes() {
    // Oracle: (1329, 28, 2) under {} AND experimentalDecorators.
    let text = "declare function d(): void;\n@d class C {}\n";
    assert_eq!(checked_rows(text), [(1329, 28, 2)]);
    assert_eq!(
        checked_rows_with(text, &legacy_decorator_options()),
        [(1329, 28, 2)]
    );
}

#[test]
fn deprecated_decorator_and_tagged_template_signatures_report_6387() {
    let text = "/** @deprecated */\n\
                    declare function dec(target: Function): void;\n\
                    @dec class C {}\n\
                    /** @deprecated */\n\
                    declare function tag(parts: any): void;\n\
                    tag`x`;\n";
    with_program_state(&[("a.ts", text)], &legacy_decorator_options(), |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 6387)
            .map(|diagnostic| {
                (
                    diagnostic.category(),
                    diagnostic.message_text().to_owned(),
                    diagnostic
                        .related
                        .iter()
                        .map(|related| related.message.code)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            [
                (
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "The signature '(target: Function): void' of 'dec' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "The signature '(parts: any): void' of 'tag' is deprecated.".to_owned(),
                    vec![2798],
                ),
            ]
        );
    });
}

#[test]
fn legacy_class_decorator_return_mismatch_reports_1270() {
    // Oracle: (1270, 38, 2) "Decorator function return type
    // 'number' is not assignable to type 'void | typeof C'".
    let text = "declare function cd(t: any): number;\n@cd class C {}\n";
    assert_eq!(
        checked_rows_with(text, &legacy_decorator_options()),
        [(1270, 38, 2)]
    );
}

#[test]
fn legacy_method_decorator_key_mismatch_chains_under_1241() {
    // Oracle: (1241, 75, 2) with the 2345 string-vs-number detail
    // in the chain tail.
    let text = "declare function md(target: any, key: number, desc: any): void;\nclass C { @md m(): void {} }\n";
    assert_eq!(
        checked_rows_with(text, &legacy_decorator_options()),
        [(1241, 75, 2)]
    );
}

#[test]
fn legacy_class_decorator_fixed_arity_uses_runtime_1278() {
    let text = "declare function d(target: Function, index: number): void;\n@d\nclass C {}\n";
    let (codes, texts) = checked_chain_with(text, &legacy_decorator_options(), 1238);
    assert_eq!(codes, [1238, 1278]);
    assert_eq!(
        texts[1],
        "The runtime will invoke the decorator with 1 arguments, but the decorator expects 2."
    );
}

#[test]
fn legacy_class_decorator_rest_arity_uses_runtime_1279() {
    let text =
            "declare function d(target: Function, index: number, ...rest: any[]): void;\n@d\nclass C {}\n";
    let (codes, texts) = checked_chain_with(text, &legacy_decorator_options(), 1238);
    assert_eq!(codes, [1238, 1279]);
    assert_eq!(
            texts[1],
            "The runtime will invoke the decorator with 1 arguments, but the decorator expects at least 2."
        );
}

#[test]
fn es_method_decorator_arity_overflow_reports_1241_and_1270() {
    // Oracle: locationless 2318 (ClassMethodDecoratorContext,
    // noLib — dropped from per-file rows) + (1241, 76, 3) + (1270,
    // 77, 2): the ES arity allowance clamps to 2, md declares 3.
    // The 1270 target display `void | (() => void)` renders at
    // the 9.3b2 signature rung (union-wrapped function type).
    let text = "declare function md(target: any, key: string, desc: any): number;\nclass C { @md m(): void {} }\n";
    assert_eq!(checked_rows(text), [(1241, 76, 3), (1270, 77, 2)]);
}

#[test]
fn es_decorator_arrow_receives_contextual_call_signature() {
    let text = "@((value, context) => { context.nonexistent; return value; })\nclass C {}\ninterface ClassDecoratorContext<T> {}\n";
    assert_eq!(checked_rows(text), [(2339, 32, 11), (6133, 104, 3)]);
}

#[test]
fn es_decorator_contextual_signature_cache_is_order_independent() {
    let text = "@((value, context) => { context.nonexistent; return value; })\nclass C {}\ninterface ClassDecoratorContext<T> {}\n";
    let actual = with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let root = state.binder.source(0).root;
        let statements = match state.data_of(root) {
            NodeData::SourceFile(data) => data.statements,
            _ => unreachable!("fixture root is a source file"),
        };
        let class = state
            .nodes_of(statements)
            .into_iter()
            .find(|&node| state.kind_of(node) == SyntaxKind::ClassDeclaration)
            .expect("fixture class declaration");
        let modifiers = match state.data_of(class) {
            NodeData::ClassDeclaration(data) => data.modifiers,
            _ => unreachable!("selected a class declaration"),
        };
        let decorator = state
            .nodes_of(modifiers)
            .into_iter()
            .find(|&node| state.kind_of(node) == SyntaxKind::Decorator)
            .expect("fixture decorator");
        let signature = state
            .get_decorator_call_signature(decorator)
            .expect("decorator signature computation")
            .expect("ES class decorator signature");

        // Warm the shared isolated-signature cache through the
        // generic helper BEFORE contextual typing reaches it.
        let isolated = state
            .get_or_create_type_from_signature(signature)
            .expect("isolated decorator signature type");
        assert_eq!(
            state
                .get_signatures_of_type(isolated, SignatureKind::Call)
                .expect("call signatures"),
            [signature]
        );
        assert!(state
            .get_signatures_of_type(isolated, SignatureKind::Construct)
            .expect("construct signatures")
            .is_empty());

        state.check_source_file(0);
        rows(state)
    });
    assert_eq!(actual, [(2339, 32, 11), (6133, 104, 3)]);
}

// ---- m4-review S6/A12 pins (oracle: vendored tsc 6.0.3, noLib,
// strict defaults, 2026-07-19) ----

#[test]
fn arity_failed_generic_call_infers_failure_candidate() {
    // S6, LIVE since 7.4b: tsc reports 2554 @48 and types r as the
    // INFERRED '1' (inferSignatureInstantiationForOverloadFailure
    // runs real inference), so the noLib 2339 on toFixed follows.
    // The stub era contained the result read (the 2339 was a
    // recorded FN; pre-M6-stub the stub-filled `unknown` leaked
    // into r → 18046 FP). Oracle-pinned 2026-07-20
    // (scratchpad probe74.mjs, vendored 6.0.3 noLib).
    assert_eq!(
        checked_rows("declare function f<T>(x: T, y: T): T;\nconst r = f(1);\nr.toFixed();\n"),
        [(2554, 48, 1), (2339, 56, 7)]
    );
}

#[test]
fn argument_error_span_skips_parentheses() {
    // A12: tsc 2345 @40 len1 — errorNode =
    // getEffectiveCheckNode(arg) (76229) unwraps the parens;
    // pre-fix the span covered `(1)`.
    assert_eq!(
        checked_rows("declare function g(x: string): void;\ng((1));\n"),
        [(2345, 40, 1)]
    );
}
