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

/// The last-assignment position of every variable, parameter and binding
/// element of `text`, by the declaration's place in the text, after the
/// walk over the whole file: the product's walk, or (`exhaustive`) the walk
/// as the reference writes it, which takes the step at every node of the
/// tree in pre-order.
fn last_assignment_positions(text: &str, exhaustive: bool) -> Vec<(u32, Option<i64>)> {
    use tsc_syntax::{NodeLookup, Published, SyntaxKind};
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let root = state.binder.source(0).root;
        if exhaustive {
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                if state.mark_assignment_at(node).unwrap() {
                    let first = stack.len();
                    tsc_syntax::for_each_child(&Published, Published.node(node), |child| {
                        stack.push(child);
                        false
                    });
                    stack[first..].reverse();
                }
            }
        } else {
            state.mark_node_assignments(root).unwrap();
            assert!(state.child_scratch.is_empty());
        }
        let mut positions = Vec::new();
        for node in state.binder.source(0).arena.node_ids() {
            if !matches!(
                state.kind_of(node),
                SyntaxKind::VariableDeclaration
                    | SyntaxKind::Parameter
                    | SyntaxKind::BindingElement
            ) {
                continue;
            }
            if let Some(symbol) = state.get_symbol_of_declaration_opt(node) {
                positions.push((
                    state.pos_of(node),
                    state.links.symbol_cold().last_assignment_pos.get(symbol),
                ));
            }
        }
        positions.sort_unstable();
        positions
    })
}

/// The walk leaves out the children at which its step does nothing and
/// below which there is nothing. What it marks is what a walk over every
/// node marks: plain, compound and logical assignments, increments,
/// destructuring through arrays, objects, shorthands, spreads, defaults,
/// parentheses and non-null assertions, `for` heads, assignments in nested
/// functions and after them, exported locals, and the places the walk does
/// not enter (type nodes, interfaces, type aliases, enums).
#[test]
fn the_assignment_walk_marks_what_a_walk_over_every_node_marks() {
    let text = r#"
export {};
declare const source: any;
let plain = 0, compound = 0, logical: any, bumped = 0, dropped = 0;
let inArray = 0, inSpread: any[] = [], inObject = 0, shorthand = 0, rest: any = {}, withDefault = 0;
let parenthesized = 0, nonNull: any, loopIn: any, loopOf: any, nested = 0, later = 0, untouched = 0;
let viaExport = 0, onlyRead = 1, inType = 0, inEnum = 0, inInterface = 0, twice = 0;
function outer(parameter: number, assigned: number, { bound, alsoBound }: { bound: number; alsoBound: number }) {
    plain = 1;
    compound += parameter;
    logical ??= source;
    bumped++;
    --dropped;
    [inArray, ...inSpread] = source;
    ({ key: inObject, shorthand, ...rest } = source);
    [withDefault = 5] = source;
    (parenthesized) = 2;
    nonNull! = 3;
    for (loopIn in source) { twice = 1; }
    for (loopOf of source) { if (source) { twice = 2; } }
    assigned = onlyRead + untouched;
    bound = alsoBound;
    const inner = () => { nested = 4; };
    inner();
    later = nested;
    type Computed = { [K in keyof typeof source]: typeof inType };
    interface Shape { member: typeof inInterface }
    enum Members { First = inEnum }
    let local: Computed | Shape | Members | undefined;
    local = undefined;
    return source.property.plain;
}
export { viaExport, viaExport as renamed };
outer(plain, compound, source);
"#;
    let product = last_assignment_positions(text, false);
    let exhaustive = last_assignment_positions(text, true);
    assert_eq!(product, exhaustive);
    // The table is not empty of marks, and it has both kinds of them.
    assert!(product.iter().filter(|(_, pos)| pos.is_some()).count() >= 18);
    assert!(product.iter().any(|(_, pos)| *pos == Some(i64::MAX)));
    assert!(product
        .iter()
        .any(|(_, pos)| pos.is_some_and(|pos| pos < 0)));
    assert!(product.iter().any(|(_, pos)| pos.is_none()));
}

/// The two tests the walk makes of a child's kind, over every kind: a
/// token is visited only as an identifier below a parent that can hold an
/// assignment's target, and those parents are the twelve kinds at which
/// `get_assignment_target` goes on; a type node, an interface, a type alias
/// and an enum are not entered; every other node is.
#[test]
fn the_assignment_walk_visits_identifiers_below_target_holders_and_every_node_it_enters() {
    use tsc_syntax::SyntaxKind;
    let holders = [
        SyntaxKind::BinaryExpression,
        SyntaxKind::PrefixUnaryExpression,
        SyntaxKind::PostfixUnaryExpression,
        SyntaxKind::ForInStatement,
        SyntaxKind::ForOfStatement,
        SyntaxKind::ParenthesizedExpression,
        SyntaxKind::ArrayLiteralExpression,
        SyntaxKind::SpreadElement,
        SyntaxKind::NonNullExpression,
        SyntaxKind::SpreadAssignment,
        SyntaxKind::ShorthandPropertyAssignment,
        SyntaxKind::PropertyAssignment,
    ];
    let mut kinds = 0;
    for value in 0..=u16::MAX {
        let Some(kind) = SyntaxKind::from_u16(value) else {
            continue;
        };
        kinds += 1;
        assert_eq!(
            super::holds_assignment_targets(kind),
            holders.contains(&kind),
            "{kind:?}"
        );
        for below in [false, true] {
            let visits = super::assignment_walk_visits(kind, below);
            let expected = if value <= SyntaxKind::LastToken.value() {
                kind == SyntaxKind::Identifier && below
            } else {
                !matches!(
                    kind,
                    SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::TypeAliasDeclaration
                        | SyntaxKind::EnumDeclaration
                ) && !crate::resolve::type_node_kind(kind)
            };
            assert_eq!(visits, expected, "{kind:?} below a holder: {below}");
        }
    }
    assert!(kinds > 300);
    // A type keyword is a token and a type node: not visited either way.
    assert!(!super::assignment_walk_visits(
        SyntaxKind::StringKeyword,
        true
    ));
    assert!(super::assignment_walk_visits(
        SyntaxKind::ExportSpecifier,
        false
    ));
}

/// `isEvolvingArrayOperationTarget` reads a property name by its identity:
/// `length` wherever it is read, `push` and `unshift` only as the callee of
/// a call, no other name, and an element assignment only with a numeric
/// index.
#[test]
fn the_evolving_array_test_knows_length_push_and_unshift_by_name_identity() {
    let text = "declare let x: any;\n\
        x.length;\n\
        x.push(1);\n\
        x.unshift(1);\n\
        x.push;\n\
        x.pop();\n\
        x.lengths;\n\
        (x).length;\n\
        x[0] = 1;\n\
        x[\"key\"] = 1;\n\
        x;\n";
    let expected = [
        false, // the declaration's own name
        true, true, true, false, false, false, true, true, false, false,
    ];
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let mut references: Vec<_> = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .filter(|&node| {
                state.kind_of(node) == tsc_syntax::SyntaxKind::Identifier
                    && state.escaped_text_of(Some(node)) == Some("x")
            })
            .collect();
        references.sort_by_key(|&node| state.pos_of(node));
        let answers: Vec<bool> = references
            .into_iter()
            .map(|node| state.is_evolving_array_operation_target(node).unwrap())
            .collect();
        assert_eq!(answers, expected);
    });
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
    use super::{CheckerState, FlowArena, FlowData, FlowFlags};
    use tsc_binder::flow::FlowPayload;

    with_program_state(
        &[("a.ts", "let value = 1;\n")],
        &CompilerOptions::default(),
        |state| {
            let mut arena = FlowArena::for_bind();
            let start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
            let other_start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
            let label = arena.create_branch_label();
            arena.add_antecedent(label, start);
            let inner = arena.create_reduce_label(label, vec![other_start], start);
            let mut outer = Vec::new();
            for count in [0, 1, 4, 5, 9] {
                outer.push(arena.create_reduce_label(label, vec![start; count], start));
            }
            arena.freeze();
            let read = |state: &CheckerState<'_>| {
                state.flow_label_antecedents_from(0, &arena, label, arena.flow(label))
            };
            for origin in outer {
                let FlowData::ReduceLabel { antecedents, .. } = arena.payload(origin) else {
                    unreachable!("fixture reduce label");
                };
                state.replace_reduce_label((0, label), Some(origin));
                let snapshot = read(state);
                let saved = state.replace_reduce_label((0, label), Some(inner));
                assert!(std::ptr::eq(snapshot, antecedents));
                assert_eq!(read(state), &[other_start]);
                state.replace_reduce_label((0, label), saved);
                assert_eq!(read(state), antecedents);
                state.replace_reduce_label((0, label), None);
                assert_eq!(snapshot, antecedents);
                assert_eq!(read(state), arena.antecedents(label));
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

fn assignment_test_type(
    state: &mut crate::state::CheckerState<'_>,
    name: &str,
) -> tsc_types::TypeId {
    let reference = state
        .binder
        .source(0)
        .arena
        .node_ids()
        .find_map(|node| {
            (state.kind_of(node) == tsc_syntax::SyntaxKind::Identifier
                && state.escaped_text_of(Some(node)) == Some(name))
            .then(|| state.parent_of(node))
            .flatten()
            .filter(|&parent| state.kind_of(parent) == tsc_syntax::SyntaxKind::TypeReference)
        })
        .unwrap();
    state.get_type_from_type_node(reference).unwrap()
}

#[test]
fn assignment_union_membership_avoids_resolving_unrelated_members() {
    let text = concat!(
        "type First = { kind: 'first' }; type Second = { kind: 'second' };",
        "declare let first: First; declare let second: Second;"
    );
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let first = assignment_test_type(state, "First");
        let second = assignment_test_type(state, "Second");
        let union = state
            .get_union_type_ex(&[first, second], tsc_types::UnionReduction::None)
            .unwrap();
        let before = (
            state.tables.len(),
            state.signatures.len(),
            state.binder.transient_symbol_count(),
            state.total_instantiation_count,
            state
                .relations
                .cache(crate::relate::RelationKind::Assignable)
                .len(),
        );
        assert!(state.type_maybe_assignable_to(union, second).unwrap());
        assert_eq!(
            before,
            (
                state.tables.len(),
                state.signatures.len(),
                state.binder.transient_symbol_count(),
                state.total_instantiation_count,
                state
                    .relations
                    .cache(crate::relate::RelationKind::Assignable)
                    .len()
            )
        );
        assert!(!state.is_type_assignable_to(first, second).unwrap());
        assert!(
            state
                .relations
                .cache(crate::relate::RelationKind::Assignable)
                .len()
                > before.4
        );
    });
}

#[test]
fn assignment_union_membership_misses_keep_structural_and_literal_relations() {
    let text = concat!(
        "type First = { kind: 'first' }; type Second = { kind: 'second' };",
        "type Compatible = { kind: 'first'; extra?: boolean }; type Missing = { kind: 'missing' };",
        "declare let first: First; declare let second: Second;",
        "declare let compatible: Compatible; declare let missing: Missing;"
    );
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let first = assignment_test_type(state, "First");
        let second = assignment_test_type(state, "Second");
        let compatible = assignment_test_type(state, "Compatible");
        let missing = assignment_test_type(state, "Missing");
        let union = state
            .get_union_type_ex(&[first, second], tsc_types::UnionReduction::None)
            .unwrap();
        assert!(state.type_maybe_assignable_to(union, compatible).unwrap());
        assert!(!state.type_maybe_assignable_to(union, missing).unwrap());
        let a = state.tables.get_string_literal_type("a");
        let b = state.tables.get_string_literal_type("b");
        let absent = state.tables.get_string_literal_type("absent");
        let fresh = state.tables.get_fresh_type_of_literal_type(b);
        let literals = state
            .get_union_type_ex(&[a, b], tsc_types::UnionReduction::None)
            .unwrap();
        assert!(state.type_maybe_assignable_to(literals, fresh).unwrap());
        assert!(!state.type_maybe_assignable_to(literals, absent).unwrap());
        assert!(state.type_maybe_assignable_to(b, fresh).unwrap());
    });
}

#[test]
fn assignment_union_membership_uses_the_unions_active_order() {
    for stable in [false, true] {
        let options = CompilerOptions {
            stable_type_ordering: Some(stable),
            ..CompilerOptions::default()
        };
        with_program_state(&[("a.ts", "")], &options, |state| {
            let z = state.tables.get_string_literal_type("z");
            let a = state.tables.get_string_literal_type("a");
            let union = state
                .get_union_type_ex(&[z, a], tsc_types::UnionReduction::None)
                .unwrap();
            let expected = if stable { [a, z] } else { [z, a] };
            assert_eq!(state.tables.members_of(union), expected);
            assert!(state.type_maybe_assignable_to(union, a).unwrap());
            assert!(state.type_maybe_assignable_to(union, z).unwrap());
            let absent = state.tables.get_string_literal_type("absent");
            assert!(!state.type_maybe_assignable_to(union, absent).unwrap());
        });
    }
}

#[test]
fn optional_chain_receivers_match_the_file_backed_walk() {
    use tsc_binder::node_util::{expression_of, is_optional_chain};
    use tsc_syntax::{NodeId, NodeLookup, Published, SyntaxKind};

    fn receivers(tree: &impl NodeLookup, mut node: NodeId) -> Vec<NodeId> {
        let mut visits = Vec::new();
        while is_optional_chain(tree, node) {
            node = expression_of(tree, node).unwrap();
            visits.push(node);
        }
        visits
    }

    let text = "export {}; declare const a: any; a.b; a?.b!.c; a?.['b'].c; a.b?.();";
    with_program_state(
        &[("one.ts", text), ("two.ts", text)],
        &CompilerOptions::default(),
        |state| {
            for file in 0..2 {
                let tree = state.binder.source(file);
                let base = tree
                    .arena
                    .node_ids()
                    .find(|&id| {
                        state.kind_of(id) == SyntaxKind::Identifier
                            && state.escaped_text_of(Some(id)) == Some("a")
                    })
                    .unwrap();
                let mut query = super::FlowQuery::for_memo_test(state, base);
                query.synthetic_props =
                    Some(vec![tsc_types::EscapedName::from_identifier_escaped_text(
                        "b",
                    )]);
                let mut synthetic_hit = false;
                let mut plain_hit = false;
                for node in tree.arena.node_ids() {
                    let visits = receivers(tree, node);
                    assert_eq!(receivers(&Published, node), visits);
                    let plain = visits
                        .iter()
                        .any(|&id| state.is_matching_reference(id, base).unwrap());
                    let synthetic = visits.iter().any(|&id| {
                        state
                            .real_node_matches_synthetic_chain(
                                id,
                                base,
                                query.synthetic_props.as_deref().unwrap(),
                                false,
                            )
                            .unwrap()
                    });
                    assert_eq!(
                        state.optional_chain_contains_reference(node, base).unwrap(),
                        plain
                    );
                    assert_eq!(
                        state
                            .optional_chain_contains_query_reference(node, &query)
                            .unwrap(),
                        synthetic
                    );
                    plain_hit |= plain;
                    synthetic_hit |= synthetic;
                }
                assert!(plain_hit && synthetic_hit);
            }
        },
    );
}

#[test]
fn loop_keys_preserve_decimal_prefix_and_dotted_suffix_equality() {
    use super::{FlowCacheKey, FlowKeyPrefix, FlowQuery};
    use tsc_syntax::SyntaxKind;
    use tsc_types::{EscapedName, JsString};

    let text = r#"declare let x: any; declare const k: any; const {p} = x; class C { field!: typeof this; }
        x; (x!); x.a.b; x["a.b"]; x[k]; x["@"]; x["\uD800"]; this;"#;
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let tree = state.binder.source(0);
        let mut expressions: Vec<_> = tree
            .arena
            .node_ids()
            .filter_map(|id| state.record_of(id).as_expression_statement()?.expression)
            .collect();
        expressions.sort_by_key(|&node| state.pos_of(node));
        let type_this = tree
            .arena
            .node_ids()
            .find(|&id| state.is_this_in_type_query(id))
            .unwrap();
        let [x, wrapper, path, dotted, indexed, at, surrogate, this] = expressions[..] else {
            panic!("expected eight expression statements")
        };
        let pattern = tree
            .arena
            .node_ids()
            .find(|&id| state.kind_of(id) == SyntaxKind::ObjectBindingPattern)
            .unwrap();
        let argument = state
            .record_of(indexed)
            .as_element_access_expression()
            .unwrap()
            .argument_expression
            .unwrap();
        let symbol = state.get_resolved_symbol(x).unwrap().unwrap().index();
        let index_symbol = state
            .get_resolved_symbol(argument)
            .unwrap()
            .unwrap()
            .index();
        let declared = state.tables.intrinsics.string;
        let initial = state.tables.intrinsics.number;
        let mut pairs = Vec::new();
        for container in [None, Some(tree.root)] {
            let prefix = container.map_or("-1".to_owned(), |id| id.index().to_string());
            let d = declared.index();
            let i = initial.index();
            let base = format!("{prefix}|{d}|{i}|{symbol}");
            for (node, suffix) in [
                (x, "".into()),
                (wrapper, "".into()),
                (path, ".a.b".into()),
                (dotted, ".a.b".into()),
                (indexed, format!(".@{index_symbol}").into()),
                (at, ".@".into()),
                (surrogate, JsString::from_code_units(&[0x2e, 0xd800])),
            ] {
                let key = state
                    .get_flow_cache_key(node, declared, initial, container)
                    .unwrap()
                    .unwrap();
                let mut expected = JsString::from(base.as_str());
                expected.push_js(suffix.as_js());
                pairs.push((key, expected));
            }
            let this_key = state
                .get_flow_cache_key(this, declared, initial, container)
                .unwrap()
                .unwrap();
            assert_eq!(
                state
                    .get_flow_cache_key(type_this, declared, initial, container)
                    .unwrap(),
                Some(this_key.clone())
            );
            pairs.push((this_key, format!("0|{prefix}|{d}|{i}").into()));
            let pattern_key = state
                .get_flow_cache_key(pattern, declared, initial, container)
                .unwrap()
                .unwrap();
            pairs.push((pattern_key, format!("{}#{d}", pattern.index()).into()));
            let dynamic = format!("@{index_symbol}");
            let other = state
                .get_flow_cache_key(argument, declared, initial, container)
                .unwrap()
                .unwrap();
            pairs.push((other, format!("{prefix}|{d}|{i}|{index_symbol}").into()));
            for props in [
                vec![dynamic.as_str()],
                vec![],
                vec!["a", "b"],
                vec!["a.b"],
                vec!["@"],
                vec![""],
            ] {
                let mut query = FlowQuery::for_memo_test(state, x);
                query.flow_container = container;
                query.declared_type = declared;
                query.initial_type = initial;
                query.synthetic_props = Some(
                    props
                        .iter()
                        .map(|s| EscapedName::from_identifier_escaped_text(s))
                        .collect(),
                );
                let key = state.get_or_set_cache_key(&mut query).unwrap().unwrap();
                assert_eq!(
                    state.get_or_set_cache_key(&mut query).unwrap(),
                    Some(key.clone())
                );
                pairs.push((key, format!("{base}.{}", props.join(".")).into()));
            }
        }
        // These numeric positions can spell the same prefix across root kinds.
        pairs.push((
            FlowCacheKey::Root(FlowKeyPrefix::Fields([0, 1, 2, 3])),
            "0|1|2|3".into(),
        ));
        pairs.push((
            FlowCacheKey::this(
                Some(tsc_syntax::NodeId::new(1)),
                tsc_types::TypeId::new(2),
                tsc_types::TypeId::new(3),
            ),
            "0|1|2|3".into(),
        ));
        for (key, old) in &pairs {
            let (prefix, suffix) = match key {
                FlowCacheKey::Root(prefix) => (prefix, None),
                FlowCacheKey::Access(access) => (&access.0, Some(&access.1)),
            };
            let mut spelling = JsString::from(match prefix {
                FlowKeyPrefix::Fields(fields) => fields
                    .map(|field| {
                        if field == u32::MAX {
                            "-1".to_string()
                        } else {
                            field.to_string()
                        }
                    })
                    .join("|"),
                FlowKeyPrefix::Node(node, ty) => format!("{}#{}", node.index(), ty.index()),
            });
            if let Some(suffix) = suffix {
                spelling.push_js(suffix.as_js());
            }
            assert_eq!(&spelling, old);
            for (other, other_old) in &pairs {
                assert_eq!(key == other, old == other_old, "{old:?}, {other_old:?}");
            }
        }
        let mut cache = rustc_hash::FxHashMap::default();
        let mut legacy = rustc_hash::FxHashMap::default();
        for (index, (key, old)) in pairs.into_iter().enumerate() {
            let ty = tsc_types::TypeId::new(index as u32);
            assert_eq!(cache.insert(key, ty), legacy.insert(old, ty));
        }
    });
}

#[test]
fn reachability_bypasses_a_shared_answer_under_a_reduce_label() {
    use tsc_binder::flow::{FlowData, FlowId};
    use tsc_types::FlowFlags;

    let text = "function f(b: boolean) { try { if (b) return 1; } finally { b; } }";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let arena = state.binder.flow_arena(0);
        let ids = || (0..arena.len() as u32).map(FlowId::new);
        let (origin, label) = ids()
            .find_map(|id| match arena.payload(id) {
                FlowData::ReduceLabel { target, .. } => Some((id, target)),
                _ => None,
            })
            .unwrap();
        let shared = ids()
            .find(|&id| arena.flow(id).flags.intersects(FlowFlags::SHARED))
            .unwrap();
        state.replace_reduce_label((0, label), Some(origin));
        let expected = state
            .is_reachable_flow_node_worker(0, shared, false)
            .unwrap();
        state.flow_node_reachable.insert((0, shared), !expected);
        state.last_flow_node = None;
        assert_eq!(
            state
                .is_reachable_flow_node_worker(0, shared, false)
                .unwrap(),
            expected
        );
        state.replace_reduce_label((0, label), None);
        state.last_flow_node = None;
        assert_eq!(
            state
                .is_reachable_flow_node_worker(0, shared, false)
                .unwrap(),
            !expected
        );
    });
}

#[test]
fn synthetic_reference_matches_the_real_path_and_its_prefixes() {
    use tsc_syntax::SyntaxKind;
    use tsc_types::EscapedName;

    let text =
        "declare let root: any, other: any; root; root.a; root.a.b; other.a.b; (root.a.b = other);";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let tree = state.binder.source(0);
        let mut targets: Vec<_> = tree
            .arena
            .node_ids()
            .filter_map(|id| state.record_of(id).as_expression_statement()?.expression)
            .collect();
        targets.sort_by_key(|&id| state.pos_of(id));
        let [root, prefix, whole, other, assignment] = targets[..] else {
            panic!("five probe expressions")
        };
        let mut query = super::FlowQuery::for_memo_test(state, root);
        query.synthetic_props = Some(
            ["a", "b"]
                .map(EscapedName::from_identifier_escaped_text)
                .to_vec(),
        );
        for (target, matches, contains) in [
            (root, false, true),
            (prefix, false, true),
            (whole, true, false),
            (other, false, false),
            (assignment, true, false),
        ] {
            assert_eq!(state.is_matching_reference(whole, target).unwrap(), matches);
            assert_eq!(
                state.is_matching_query_reference(&query, target).unwrap(),
                matches
            );
            assert_eq!(
                state.contains_matching_reference(whole, target).unwrap(),
                contains
            );
            assert_eq!(
                state
                    .contains_matching_query_reference(&query, target)
                    .unwrap(),
                contains
            );
            assert_eq!(
                state
                    .query_reference_receiver_matches(&query, target)
                    .unwrap(),
                state.is_matching_reference(prefix, target).unwrap()
            );
        }

        query.synthetic_props = None;
        let symbol = state.get_resolved_symbol(root).unwrap().unwrap();
        query.memo_key = state.flow_memo_key(&mut query);
        assert!(query.identifier_location.is_some());
        assert!(!state
            .contains_matching_query_reference(&query, root)
            .unwrap());
        state.links.revert_node_resolved_symbol_late_bind(root);
        let target = state
            .record_of(prefix)
            .as_property_access_expression()
            .unwrap()
            .expression
            .unwrap();
        assert_eq!(state.kind_of(target), SyntaxKind::Identifier);
        assert!(state.is_matching_query_reference(&query, target).unwrap());
        assert_eq!(
            state
                .links
                .read_node(root, |links| links.resolved_symbol.resolved()),
            Some(symbol)
        );
    });
}

#[test]
fn narrowing_uses_the_receiver_or_left_operand_for_presence() {
    use tsc_syntax::SyntaxKind;
    use tsc_types::TypeFacts;
    let source = "declare let x: any; x ?? x; x ??= x; x?.p; x?.[0]; x?.(); x?.p!.q;";
    with_program_state(
        &[("a.ts", source)],
        &CompilerOptions {
            strict_null_checks: Some(true),
            ..Default::default()
        },
        |state| {
            let tree = state.binder.source(0);
            let zero = state.tables.get_number_literal_type(0.0);
            let one = state.tables.get_number_literal_type(1.0);
            let ty = state
                .get_union_type_ex(
                    &[zero, one, state.tables.intrinsics.null],
                    tsc_types::UnionReduction::Literal,
                )
                .unwrap();
            let mut operands = Vec::new();
            for node in tree.arena.node_ids() {
                let record = state.record_of(node);
                if let Some(binary) = record.as_binary_expression() {
                    operands.push((binary.left.unwrap(), true));
                    operands.push((binary.right.unwrap(), false));
                } else if let Some(non_null) = record.as_non_null_expression() {
                    operands.push((non_null.expression.unwrap(), false));
                } else if record.question_dot_token_field().is_some()
                    && matches!(
                        record.kind,
                        SyntaxKind::PropertyAccessExpression
                            | SyntaxKind::ElementAccessExpression
                            | SyntaxKind::CallExpression
                    )
                {
                    operands.push((record.expression_field().unwrap(), true));
                }
            }
            assert_eq!(operands.len(), 9);
            for (expr, presence) in operands {
                let mut query = super::FlowQuery::for_memo_test(state, expr);
                for positive in [true, false] {
                    let facts = match (presence, positive) {
                        (true, true) => TypeFacts::NE_UNDEFINED_OR_NULL,
                        (true, false) => TypeFacts::EQ_UNDEFINED_OR_NULL,
                        (false, true) => TypeFacts::TRUTHY,
                        (false, false) => TypeFacts::FALSY,
                    };
                    let expected = state.get_adjusted_type_with_facts(ty, facts).unwrap();
                    let counts = |state: &crate::state::CheckerState| {
                        let c = crate::CheckerCounts::of(state, true);
                        (
                            c.types,
                            c.instantiations,
                            c.symbols,
                            state.diagnostics.iter().count(),
                        )
                    };
                    let before = counts(state);
                    assert_eq!(
                        state.narrow_type(&mut query, ty, expr, positive).unwrap(),
                        expected
                    );
                    assert_eq!(counts(state), before);
                }
            }
            let mut query = super::FlowQuery::for_memo_test(state, tree.root);
            assert!(state.parent_of(tree.root).is_none());
            assert_eq!(
                state.narrow_type(&mut query, ty, tree.root, true).unwrap(),
                ty
            );
        },
    );
}
