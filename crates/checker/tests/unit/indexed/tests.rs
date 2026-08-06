use tsc_types::{AccessFlags, CompilerOptions, IndexFlags, SymbolFlags, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

fn annotation_type(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let annotation =
        find_probe_annotation(state.binder.source(0), name).expect("var with annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("annotation resolves")
}

fn type_parameter(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let source = state.binder.source(0);
    let inside = source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == tsc_syntax::SyntaxKind::VariableDeclaration)
        .expect("var declaration");
    let symbol = state
        .resolve_name(
            Some(inside),
            name,
            SymbolFlags::TYPE_PARAMETER,
            None,
            false,
            false,
        )
        .expect("resolve_name")
        .expect("type parameter resolves");
    state.get_declared_type_of_type_parameter(symbol)
}

#[test]
fn keyof_type_literal_yields_the_literal_union() {
    with_program_state(
        &[(
            "a.ts",
            "declare var v: keyof { a: string; \"1\": number };\ndeclare var w: \"a\" | \"1\";\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let keyof = annotation_type(state, "v");
            let expected = annotation_type(state, "w");
            // Oracle-pinned: string-named `"1"` keys are STRING
            // literals in keyof.
            assert_eq!(keyof, expected);
        },
    );
}

#[test]
fn keyof_with_string_index_signature_widens_to_string_or_number() {
    with_program_state(
        &[(
            "a.ts",
            "declare var v: keyof { [k: string]: any };\ndeclare var w: string | number;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let keyof = annotation_type(state, "v");
            let expected = annotation_type(state, "w");
            assert_eq!(keyof, expected);
        },
    );
}

#[test]
fn keyof_interface_carries_an_index_origin() {
    with_program_state(
        &[(
            "a.ts",
            "interface I { a: string; b: number }\ndeclare var v: keyof I;\ndeclare var w: \"a\" | \"b\";\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let keyof = annotation_type(state, "v");
            let plain = annotation_type(state, "w");
            // The interface keyof denormalizes through an origin
            // index type: a DISTINCT union interned under the `#`
            // key, structurally equal to the plain literal union.
            assert_ne!(keyof, plain);
            let TypeData::Union { origin, .. } = state.tables.type_of(keyof).data.clone()
            else {
                panic!("keyof I is a union");
            };
            let origin = origin.expect("keyof I carries an origin");
            assert!(matches!(
                state.tables.type_of(origin).data,
                TypeData::Index { .. }
            ));
            assert_eq!(state.is_type_assignable_to(keyof, plain), Ok(true));
            assert_eq!(state.is_type_assignable_to(plain, keyof), Ok(true));
        },
    );
}

#[test]
fn keyof_generic_defers_and_instantiates() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var v: keyof T; var w: { a: string }; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let keyof = annotation_type(state, "v");
            assert!(state.tables.flags_of(keyof).intersects(TypeFlags::INDEX));
            let t = type_parameter(state, "T");
            assert!(matches!(
                state.tables.type_of(keyof).data,
                TypeData::Index { ty, .. } if ty == t
            ));
            // The per-operand cache interns the deferred index type.
            let again = state
                .get_index_type(t, IndexFlags::NONE)
                .expect("index type");
            assert_eq!(again, keyof);
            // Instantiation maps through to the literal key.
            let literal_object = annotation_type(state, "w");
            let mapper = state.create_type_mapper(vec![t], Some(vec![literal_object]));
            let instantiated = state
                .instantiate_type(keyof, Some(mapper))
                .expect("instantiation");
            assert_eq!(instantiated, state.tables.get_string_literal_type("a"));
        },
    );
}

#[test]
fn indexed_access_reads_properties_and_unions() {
    with_program_state(
        &[(
            "a.ts",
            "declare var v: { a: string; b: number }[\"a\"];\n\
             declare var y: { a: string; b: number }[\"a\" | \"b\"];\n\
             declare var z: string | number;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            assert_eq!(annotation_type(state, "v"), state.tables.intrinsics.string);
            let union_access = annotation_type(state, "y");
            let expected = annotation_type(state, "z");
            assert_eq!(union_access, expected);
        },
    );
}

#[test]
fn tuple_indexed_access_reads_the_synthesized_members() {
    with_program_state(
        &[(
            "a.ts",
            // Array<T> feeds getTupleBaseType (the tuple target's
            // base) during member resolution.
            "interface Array<T> { length: number }\n\
             declare var v: [string, number][1];\ndeclare var w: [string, number][\"length\"];\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // 5.3c: the property lookup consults the tuple
            // reference's synthesized per-index/length members.
            assert_eq!(annotation_type(state, "v"), state.tables.intrinsics.number);
            let length = annotation_type(state, "w");
            // Fixed [string, number]: length is the literal 2.
            assert!(state
                .tables
                .flags_of(length)
                .intersects(TypeFlags::NUMBER_LITERAL));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_indexed_access_defers_instantiates_and_constrains() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends { a: string }>() { var v: T[\"a\"]; var w: { a: \"x\" }; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let access = annotation_type(state, "v");
            assert!(state
                .tables
                .flags_of(access)
                .intersects(TypeFlags::INDEXED_ACCESS));
            let t = type_parameter(state, "T");
            assert!(matches!(
                state.tables.type_of(access).data,
                TypeData::IndexedAccess { object_type, .. } if object_type == t
            ));
            // Interned per (object, index, flags).
            let index = state.tables.get_string_literal_type("a");
            let again = state
                .get_indexed_access_type(t, index, AccessFlags::NONE, None, None, None)
                .expect("indexed access");
            assert_eq!(again, access);
            // The base constraint re-accesses through the bounds.
            let constraint = state
                .get_base_constraint_of_type(access)
                .expect("constraint in slice");
            assert_eq!(constraint, Some(state.tables.intrinsics.string));
            // Instantiation maps through the concrete object.
            let concrete = annotation_type(state, "w");
            let mapper = state.create_type_mapper(vec![t], Some(vec![concrete]));
            let instantiated = state
                .instantiate_type(access, Some(mapper))
                .expect("instantiation");
            assert_eq!(instantiated, state.tables.get_string_literal_type("x"));
        },
    );
}

#[test]
fn keyof_distributes_over_unions_and_intersections() {
    with_program_state(
        &[(
            "a.ts",
            "declare var v: keyof ({ a: string; b: number } | { b: string; c: number });\n\
             declare var w: keyof ({ a: string } & { b: number });\n\
             declare var u: \"a\" | \"b\";\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // Union operand -> intersection of key sets = "b".
            let of_union = annotation_type(state, "v");
            assert_eq!(of_union, state.tables.get_string_literal_type("b"));
            // Intersection operand -> union of key sets.
            let of_intersection = annotation_type(state, "w");
            let expected = annotation_type(state, "u");
            assert_eq!(of_intersection, expected);
        },
    );
}

// ---- m4-review S1/S3 pins (oracle: vendored tsc 6.0.3, noLib,
// strict defaults, 2026-07-19) ----

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

#[test]
fn double_underscore_element_access_resolves() {
    // S1: tsc clean. Pre-fix the raw `__x` propName missed the
    // escaped-keyed member table → 7053.
    assert_eq!(
        checked_rows("interface O { __x: number }\ndeclare const o: O;\no[\"__x\"];\n"),
        []
    );
}

#[test]
fn double_underscore_indexed_access_type_resolves() {
    // S1: tsc clean — V is number (pre-fix: 2339 + errorType, so
    // the assignment below would misreport).
    assert_eq!(
        checked_rows(
            "interface O { __x: number }\ntype V = O[\"__x\"];\ndeclare const v: V;\nconst n: number = v;\n"
        ),
        []
    );
}

#[test]
fn double_underscore_suggestion_args_stay_escaped() {
    // S1: tsc 2551 @54 len8 with arg0 = '___helo' — the ESCAPED
    // propName VERBATIM (tsc passes the __String straight through)
    // — and the suggestion '__hello' unescaped.
    let text = "interface P { __hello: number }\ndeclare const p: P;\np[\"__helo\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let rows: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .collect();
        assert_eq!(rows.len(), 1, "{rows:#?}");
        let diagnostic = rows[0];
        assert_eq!(
            (diagnostic.code(), diagnostic.start, diagnostic.length),
            (2551, Some(54), Some(8))
        );
        assert!(
            diagnostic.message.text.contains("'___helo'")
                && diagnostic.message.text.contains("'__hello'"),
            "{}",
            diagnostic.message.text
        );
    });
}

#[test]
fn indexed_access_missing_property_uses_raw_string_literal_value() {
    // getPropertyTypeForIndexType passes indexType.value directly
    // to TS2339. The diagnostic template already uses single
    // quotes, so double quotes inside the property value are not
    // escaped a second time.
    let text = "declare module \"ambientModule\" {\n\
                    export type typ = 1;\n\
                    export var val: typ;\n\
                }\n\
                type Bad = (typeof globalThis)[\"\\\"ambientModule\\\"\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let rows: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2339)
            .collect();
        assert_eq!(rows.len(), 1, "{rows:#?}");
        assert_eq!(
            rows[0].message_text(),
            "Property '\"ambientModule\"' does not exist on type 'typeof globalThis'."
        );
    });
}

#[test]
fn keyof_typeof_enum_excludes_the_reverse_map_number() {
    // S3: tsc clean — enumNumberIndexInfo is excluded from
    // getLiteralTypeFromProperties, so K = "A" | "B" and the
    // string assignment holds.
    assert_eq!(
        checked_rows(
            "enum E { A, B }\ntype K = keyof typeof E;\ndeclare const k: K;\nconst s: string = k;\n"
        ),
        []
    );
}

#[test]
fn keyof_typeof_enum_rejects_number() {
    // S3 reverse direction: tsc 2322 @72 len2 (Type 'number' is
    // not assignable to type '"A" | "B"') — pre-fix the leaked
    // number index made this assignment pass.
    assert_eq!(
        checked_rows(
            "enum E { A, B }\ntype K = keyof typeof E;\ndeclare const n: number;\nconst k2: K = n;\n"
        ),
        [(2322, 72, 2)]
    );
}

#[test]
fn checked_js_publishes_implicit_any_index_diagnostic() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "/** @type {string} */\n\
                   const key = \"missing\";\n\
                   const object = { known: 1 };\n\
                   object[key];\n"
                .to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 7053)
            .count(),
        1
    );
}

#[test]
fn checked_js_empty_this_assignment_uses_widened_index_error_face() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "this[\"known\"] = {};\n\
                   this[\"known\"][\"missing\"] = {};\n"
                .to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 7053)
        .map(|diagnostic| {
            (
                diagnostic.start,
                diagnostic.length,
                diagnostic.message.text.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            Some(20),
            Some(24),
            "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type '{}'."
                .to_owned(),
        )]
    );
}

#[test]
fn checked_js_jsdoc_index_carrier_stays_private() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "/** @type {Object.<string, string>} */\n\
                   const object = { known: \"value\" };\n\
                   object[\"missing\"] = \"value\";\n"
                .to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 7053));
}

#[test]
fn jsdoc_template_prototype_index_carriers_keep_their_annotations() {
    for (fixture, text) in [
        (
            "jsdocTemplateTag4",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../ts-tests/tests/cases/conformance/jsdoc/jsdocTemplateTag4.ts"
            )),
        ),
        (
            "jsdocTemplateTag5",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../ts-tests/tests/cases/conformance/jsdoc/jsdocTemplateTag5.ts"
            )),
        ),
    ] {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: text.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            },
        );
        let rows = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 7053)
            .map(|diagnostic| (diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>();
        assert_eq!(rows, [], "{fixture}: {:#?}", result.diagnostics);
    }
}

#[test]
fn checked_js_late_bound_class_member_stays_private() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "const key = \"member\";\n\
                   class C {\n\
                     constructor() { this[key] = 1; }\n\
                     read() { return this[key]; }\n\
                   }\n"
            .to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 7053));
}

#[test]
fn unchecked_js_does_not_publish_implicit_any_index_diagnostic() {
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "/** @type {string} */\n\
                   const key = \"missing\";\n\
                   const object = { known: 1 };\n\
                   object[key];\n"
                .to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(false),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 7053));
}
