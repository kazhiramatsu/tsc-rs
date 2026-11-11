use super::FlowType;
use crate::state::test_support::with_program_state;
use crate::CompilerOptions;

#[test]
fn assignment_walk_keeps_the_last_write_definite_assignment_and_scratch_prefix() {
    with_program_state(
        &[("a.ts", "function f() { let x: number; x = 1; x += 2; }")],
        &CompilerOptions::default(),
        |state| {
            let root = state.binder.source(0).root;
            let last = state
                .binder
                .source(0)
                .arena
                .node_ids()
                .filter(|&node| {
                    state.kind_of(node) == tsc_syntax::SyntaxKind::Identifier
                        && state.escaped_text_of(Some(node)) == Some("x")
                })
                .max_by_key(|&node| state.pos_of(node))
                .unwrap();
            let symbol = state.get_resolved_symbol(last).unwrap().unwrap();
            let declaration = state.binder.symbol(symbol).value_declaration.unwrap();
            let expected = state.extend_assignment_position(last, declaration);
            state.child_scratch.push(root);
            state.mark_node_assignments(root).unwrap();
            assert_eq!(state.child_scratch, [root]);
            assert_eq!(
                state.links.symbol_cold().last_assignment_pos.get(symbol),
                Some(-expected)
            );
        },
    );
}

#[test]
fn flow_type_accessors() {
    let ty = tsc_types::TypeId::new(7);
    assert_eq!(FlowType::Type(ty).get_type(), ty);
    assert_eq!(FlowType::Incomplete(ty).get_type(), ty);
    assert!(!FlowType::Type(ty).is_incomplete());
    assert!(FlowType::Incomplete(ty).is_incomplete());
}

#[test]
fn class_property_implicit_any_prints_the_declaration_name() {
    // symbolToString prints the declaration name (`#p`), never
    // the `__#<id>@` mangling (56202/56218; 6.6 review C1;
    // oracle-pinned vs vendored tsc 6.0.3 noLib — the
    // no-assignment auto face; the `= []` autoArray flavor needs
    // the Array global and rides the lib-backed band).
    let text = "class C { #p; constructor() { } }\n";
    assert_eq!(checked_rows(text), [(7008, 10, 2)]);
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let message = &state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error)
            .expect("7008 row")
            .message;
        assert!(
            message.text.contains("'#p'"),
            "declaration-name display: {}",
            message
                .text
                .as_str()
                .expect("scalar diagnostic observation")
        );
    });
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
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

// ---- class-property flow init (6.6e; rows oracle-pinned vs
// vendored tsc 6.0.3 noLib per shape, 2026-07-19) ----

#[test]
fn constructor_assignments_type_auto_properties() {
    // getFlowTypeInConstructor: the this-rooted synthetic query
    // over returnFlowNode infers `x: number` — the misuse row is
    // the proof the inference (not any) landed.
    assert_eq!(
        checked_rows(
            "class C { x; constructor() { this.x = 1; } }\ndeclare const c: C;\nconst s: string = c.x;\n"
        ),
        [(2322, 71, 1)]
    );
    // No assignment reaches the return flow — all-nullable answer
    // falls through to the widening tail's 7008.
    assert_eq!(
        checked_rows("class C { x; constructor() { } }\n"),
        [(7008, 10, 1)]
    );
    // Conditional assignments union (number | string here — the
    // misuse row proves the JOIN, not an any/单-type collapse).
    assert_eq!(
        checked_rows(
            "class C { x; constructor(b: boolean) { if (b) this.x = 1; else this.x = \"s\"; const v = this.x; const n: boolean = v; } }\n"
        ),
        [(2322, 101, 1)]
    );
}

#[test]
fn in_constructor_this_property_reads_flow_narrow() {
    // The access.rs face (75319→75347): a this-property READ in
    // the constructor routes through getFlowTypeOfProperty with
    // the REAL access node as reference.
    assert_eq!(
        checked_rows("class C { x; constructor() { this.x = 1; const y: string = this.x; } }\n"),
        [(2322, 47, 1)]
    );
}

#[test]
fn ungated_faces_stay_clean_on_oracle_clean_shapes() {
    // Two faces the 6.6f gate retirement UNMASKED in canaries —
    // pinned oracle-clean (vendored tsc noLib, 2026-07-19):
    // late-bound unique-literal computed key indexing…
    assert_eq!(
        checked_rows("const a = \"a\";\ntype A = { [a]: number };\ndeclare const c: A;\nc[a];\n"),
        []
    );
    // …and for-in over an optional chain narrowing the chain's
    // links in the body (tsc #51941).
    assert_eq!(
        checked_rows(
            "type R = { [k: string]: T5 };\ntype T5 = { main?: { childs: R } };\nfunction f50(obj: T5) {\n  for (const key in obj.main?.childs) {\n    if (obj.main.childs[key] === obj) { return obj; }\n  }\n  return null;\n}\n"
        ),
        []
    );
}

#[test]
fn in_operator_fixture_body_is_clean_without_lib() {
    // Without a global Record, missing-key synthesis is skipped
    // and the fixture body still checks clean.
    let text = "const a = 'a';\nconst b = 'b';\nconst d = 'd';\n\ntype A = { [a]: number; };\ntype B = { [b]: string; };\n\ndeclare const c: A | B;\n\nif ('a' in c) {\n    c;\n    c['a'];\n}\n\nif ('d' in c) {\n    c;\n}\n\nif (a in c) {\n    c;\n    c[a];\n}\n\nif (d in c) {\n    c;\n}\n";
    assert_eq!(checked_rows(text), []);
}

#[test]
fn static_blocks_and_privates_flow_type_auto_properties() {
    // getFlowTypeInStaticBlocks (this = the class in a static
    // block).
    assert_eq!(
        checked_rows(
            "class C { static x; static { this.x = 1; } }\ndeclare const n: string;\nconst z: string = C.x;\n"
        ),
        [(2322, 76, 1)]
    );
    // Private identifiers: accessName = the `__#…@` description
    // (\"#x\") matching the real `this.#x` access name.
    assert_eq!(
        checked_rows(
            "class C { #x; constructor() { this.#x = \"s\"; const n: number = this.#x; } }\n"
        ),
        [(2322, 51, 1)]
    );
}

fn strict_rows(text: &str) -> Vec<(u32, u32, u32)> {
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
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

#[test]
fn never_returning_this_call_through_a_contextual_this_parameter_ends_its_branch() {
    // assignContextualParameterTypes (78386) creates the function's
    // thisParameter with createSymbolWithType, which copies the contextual
    // parameter's declarations; getExplicitThisType (72464) then finds an
    // explicitly annotated `this`, and `this.skip()` (`skip(): never`) is an
    // effects call that ends its branch (VS Code's mocha `this.skip()`).
    let text = "interface Context { skip(): never; }\n\
                declare function setup(fn: (this: Context) => void): void;\n\
                declare function resolveTarget(): string | undefined;\n\
                let host: string;\n\
                setup(function () {\n\
                \tconst target = resolveTarget();\n\
                \tif (!target) {\n\
                \t\tthis.skip();\n\
                \t}\n\
                \thost = target;\n\
                });\n\
                export { host };\n";
    assert_eq!(strict_rows(text), []);
}

#[test]
fn exhaustive_switch_query_may_open_candidate_trials_while_computing() {
    // functionHasImplicitReturn reaches the switch clause while the arrow's
    // return type is inferred, and computeExhaustiveSwitchStatement checks
    // the switch expression (checkExpressionCached, 78946): resolving
    // `pick(x)` opens overload candidate trials while the switch's
    // in-process marker is set. The speculation transaction keeps the
    // marker set balanced instead of requiring it empty (VS Code witness).
    let text = "declare function pick(x: string): 'a' | 'b';\n\
                declare function pick(x: number): 'a' | 'b';\n\
                const g = (x: string) => {\n\
                \tswitch (pick(x)) {\n\
                \t\tcase 'a': return 1;\n\
                \t\tcase 'b': return 2;\n\
                \t}\n\
                };\n\
                export const n: number = g('');\n";
    assert_eq!(strict_rows(text), []);
}

#[test]
fn antecedent_snapshot_survives_nested_reduce_label_overrides() {
    use tsc_binder::flow::FlowId;
    with_program_state(
        &[("a.ts", "let value = 1;\n")],
        &CompilerOptions::default(),
        |state| {
            let label = FlowId::new(0);
            let original = state.flow_label_antecedents(0, label).to_vec();
            for count in [0, 1, 4, 5, 9] {
                let outer: Vec<FlowId> = (1..=count).map(FlowId::new).collect();
                state
                    .reduce_label_overrides
                    .insert((0, label), outer.clone());
                let snapshot = state.flow_label_antecedents(0, label);
                let saved = state
                    .reduce_label_overrides
                    .insert((0, label), vec![FlowId::new(99)]);
                assert_eq!(&*snapshot, outer.as_slice());
                assert_eq!(&*state.flow_label_antecedents(0, label), &[FlowId::new(99)]);
                state
                    .reduce_label_overrides
                    .insert((0, label), saved.unwrap());
                assert_eq!(&*state.flow_label_antecedents(0, label), outer.as_slice());
                state.reduce_label_overrides.remove(&(0, label));
                assert_eq!(&*snapshot, outer.as_slice());
                assert_eq!(
                    &*state.flow_label_antecedents(0, label),
                    original.as_slice()
                );
            }
        },
    );
}

#[test]
fn a_flow_node_past_two_bytes_is_read_from_the_files_overflow() {
    // 66,000 assignments give the file more flow nodes than a two-byte
    // node slot holds, so the reference after them finds its flow node in
    // the file's overflow. Narrowing by the last assignment shows it was
    // found: without a flow node `x` would be `string | number`.
    let mut text = String::from("let x: string | number = \"\";\n");
    for _ in 0..66_000 {
        text.push_str("x = 1;\n");
    }
    text.push_str("const s: string = x;\n");
    with_program_state(
        &[("a.ts", text.as_str())],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let messages = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
                .map(|diagnostic| {
                    diagnostic
                        .message
                        .text
                        .as_str()
                        .expect("scalar diagnostic observation")
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                messages,
                ["Type 'number' is not assignable to type 'string'."]
            );
        },
    );
}

#[test]
fn inequality_narrows_a_uniform_union_as_tsgo_does() {
    // tsgo flow.go:601-606 takes the compared unit out of a uniform union
    // by its position (removeType) and filters by comparability otherwise.
    // Every message is tsgo 7.1.0-dev's (aa8149273) for the same program:
    // enum members with a computed member kept, string and number literal
    // unions, `undefined` beside literals, a named union inside a union
    // (the origin survives when the removed literal is named by it), the
    // boolean pair, a two-member enum union, a number literal against an
    // enum (the comparable filter: the computed member goes too), a
    // non-unit comparand, a computed comparand and a chain of two tests.
    let text = "\
declare function f(): number;
enum E { A = 1, B = 2, C = 3, D = f() }
enum S { X = \"x\", Y = \"y\", Z = \"z\" }
type L = \"a\" | \"b\" | \"c\";
type N = 1 | 2 | 3;
type M = L | undefined;
type Named = \"p\" | \"q\";
type Outer = Named | \"r\";
declare const e: E;
declare const s: S;
declare const l: L;
declare const n: N;
declare const m: M;
declare const o: Outer;
declare const b: boolean;
declare const two: E.A | E.B;
function t1() { if (e !== E.A) { const x: never = e; } else { const y: never = e; } }
function t2() { if (e === E.B) { return; } const x: never = e; }
function t3() { if (s !== S.Y) { const x: never = s; } }
function t4() { if (l !== \"b\") { const x: never = l; } }
function t5() { if (n !== 2) { const x: never = n; } }
function t6() { if (m !== \"a\") { const x: never = m; } }
function t7() { if (o !== \"p\") { const x: never = o; } }
function t8() { if (o !== \"r\") { const x: never = o; } }
function t9() { if (b !== true) { const x: never = b; } }
function t10() { if (two !== E.A) { const x: never = two; } }
function t11() { if (e !== 1) { const x: never = e; } }
function t12() { if (l !== (\"z\" as string)) { const x: never = l; } }
function t13() { if (e !== E.D) { const x: never = e; } }
function t14(k: E.A | E.B | E.C) { if (k !== E.A && k !== E.C) { const x: never = k; } }
";
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let narrowed = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2322)
            .map(|diagnostic| {
                let message = diagnostic
                    .message
                    .text
                    .as_str()
                    .expect("scalar diagnostic observation");
                message
                    .strip_prefix("Type '")
                    .and_then(|rest| rest.strip_suffix("' is not assignable to type 'never'."))
                    .expect("an assignment to never")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            narrowed,
            [
                "E.B | E.C | E.D",
                "E.A",
                "E.A | E.C | E.D",
                "S.X | S.Z",
                "\"a\" | \"c\"",
                "1 | 3",
                "\"b\" | \"c\" | undefined",
                "\"q\" | \"r\"",
                "Named",
                "false",
                "E.B",
                "E.B | E.C",
                "L",
                "E.A | E.B | E.C",
                "E.B",
            ]
        );
    });
}
