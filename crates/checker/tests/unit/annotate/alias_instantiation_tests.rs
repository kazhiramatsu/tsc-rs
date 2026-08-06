use tsc_types::{CompilerOptions, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_of(state: &CheckerState, name: &str) -> tsc_syntax::NodeId {
    find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
}

#[test]
fn generic_alias_instantiates_with_alias_stamping_and_interning() {
    with_program_state(
        &[(
            "a.ts",
            "type A<T> = T | null;\ndeclare var v: A<string>;\ndeclare var w: A<string>;\ndeclare var u: string | null;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let a = state
                .resolve_file_scope_name("A", tsc_types::SymbolFlags::TYPE_ALIAS)
                .expect("A resolves");
            let v = annotation_of(state, "v");
            let instantiated = state.get_type_from_type_node(v).expect("A<string>");
            assert!(state
                .tables
                .flags_of(instantiated)
                .intersects(TypeFlags::UNION));
            assert_eq!(state.tables.type_of(instantiated).alias_symbol, Some(a));
            assert_eq!(
                state.tables.type_of(instantiated).alias_type_arguments.as_deref(),
                Some(&[state.tables.intrinsics.string][..])
            );
            let w = annotation_of(state, "w");
            let again = state.get_type_from_type_node(w).expect("A<string>");
            assert_eq!(again, instantiated, "alias instantiations intern");
            // The alias id participates in the union intern key: the
            // bare structural twin is a DISTINCT type, like tsc.
            let u = annotation_of(state, "u");
            let bare = state.get_type_from_type_node(u).expect("string | null");
            assert_ne!(bare, instantiated);
            // ...but relations see them as the same shape.
            assert_eq!(state.is_type_assignable_to(bare, instantiated), Ok(true));
            assert_eq!(state.is_type_assignable_to(instantiated, bare), Ok(true));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_tuple_normalization_simplifies_variadic_indexed_access_elements() {
    with_program_state(
        &[(
            "a.ts",
            "interface Object {}\ninterface Array<T> { [n: number]: T; length: number }\n\
             type G<T extends { a: [unknown]; b: [unknown] }> = [...T[\"a\" | \"b\"]];\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("G", tsc_types::SymbolFlags::TYPE_ALIAS)
                .expect("G resolves");
            let declared = state
                .get_declared_type_of_symbol_slice(symbol)
                .expect("G's generic tuple resolves");
            assert!(state.tables.is_generic_tuple_type(declared));

            let elements = state
                .get_type_arguments(declared)
                .expect("generic tuple elements resolve");
            assert_eq!(elements.len(), 1);
            let simplified_for_reading = state
                .get_simplified_type(elements[0], /*writing*/ false)
                .expect("element simplifies for reading");
            assert!(state
                .tables
                .flags_of(simplified_for_reading)
                .intersects(TypeFlags::UNION));
            let simplified_for_writing = state
                .get_simplified_type(elements[0], /*writing*/ true)
                .expect("element simplifies for writing");
            assert!(state
                .tables
                .flags_of(simplified_for_writing)
                .intersects(TypeFlags::INTERSECTION));

            let normalized = state
                .get_normalized_type(declared, /*writing*/ false)
                .expect("generic tuple normalizes");
            let TypeData::Union { types, .. } = &state.tables.type_of(normalized).data else {
                panic!("the union index should distribute the variadic tuple");
            };
            assert_eq!(types.len(), 2);
            assert!(types
                .iter()
                .all(|&member| state.tables.is_generic_tuple_type(member)));

            let normalized_for_writing = state
                .get_normalized_type(declared, /*writing*/ true)
                .expect("generic tuple normalizes for writing");
            assert_ne!(normalized_for_writing, normalized);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn alias_of_alias_restamps_the_outer_alias() {
    with_program_state(
        &[(
            "a.ts",
            "type A<T> = T | null;\ntype B = A<string>;\ndeclare var v: B;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let b = state
                .resolve_file_scope_name("B", tsc_types::SymbolFlags::TYPE_ALIAS)
                .expect("B resolves");
            let v = annotation_of(state, "v");
            let declared = state.get_type_from_type_node(v).expect("B resolves");
            assert!(state.tables.flags_of(declared).intersects(TypeFlags::UNION));
            assert_eq!(
                state.tables.type_of(declared).alias_symbol,
                Some(b),
                "the outer alias reference stamps ITS symbol on the instantiation"
            );
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn declared_alias_union_carries_the_alias_with_parameter_arguments() {
    with_program_state(
        &[(
            "a.ts",
            "function f() { type L<T> = T | null; var v: L<string>; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // No alias host on the annotation: the instantiation
            // inherits the DECLARED union's alias (L with its own
            // parameters) and instantiates the alias arguments.
            let v = annotation_of(state, "v");
            let instantiated = state.get_type_from_type_node(v).expect("L<string>");
            let alias = state
                .tables
                .type_of(instantiated)
                .alias_symbol
                .expect("inherited alias symbol");
            assert_eq!(state.binder.symbol(alias).escaped_name, "L");
            assert_eq!(
                state
                    .tables
                    .type_of(instantiated)
                    .alias_type_arguments
                    .as_deref(),
                Some(&[state.tables.intrinsics.string][..])
            );
        },
    );
}

#[test]
fn bare_generic_alias_reference_reports_2314_with_plain_display() {
    with_program_state(
        &[("a.ts", "type A<T> = T;\ndeclare var v: A;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            let rendered: Vec<(u32, String)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.message_text().to_owned()))
                .collect();
            assert_eq!(
                rendered,
                [(
                    2314,
                    "Generic type 'A' requires 1 type argument(s).".to_owned()
                )],
                "alias arity errors use the plain symbol display"
            );
        },
    );
}

#[test]
fn intrinsic_string_mapping_aliases_route_to_get_string_mapping_type() {
    with_program_state(
        &[(
            "a.ts",
            "type Uppercase<S extends string> = intrinsic;\ndeclare var v: Uppercase<\"abc\">;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let mapped = state
                .get_type_from_type_node(v)
                .expect("Uppercase<\"abc\">");
            assert_eq!(mapped, state.tables.get_string_literal_type("ABC"));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn self_referential_generic_alias_reports_2456() {
    with_program_state(
        &[("a.ts", "type A<T> = A<T>;\ndeclare var v: A<string>;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            // Oracle-pinned: tsc emits 2456 at the declaration
            // plus 2315 at BOTH references (the mid-cycle declared
            // type is errorType with no typeParameters, so each
            // argument list trips checkNoTypeArguments).
            let mut codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            codes.sort_unstable();
            assert_eq!(codes, [2315, 2315, 2456]);
        },
    );
}

#[test]
fn generic_alias_of_type_literal_stamps_the_anonymous_type() {
    with_program_state(
        &[(
            "a.ts",
            "type Box<T> = { value: T };\ndeclare var v: Box<string>;\ndeclare var w: Box<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let box_symbol = state
                .resolve_file_scope_name("Box", tsc_types::SymbolFlags::TYPE_ALIAS)
                .expect("Box resolves");
            let v = annotation_of(state, "v");
            let instantiated = state.get_type_from_type_node(v).expect("Box<string>");
            // The RHS type literal becomes an instantiated anonymous
            // shell carrying the alias.
            assert!(matches!(
                state.tables.type_of(instantiated).data,
                TypeData::Object
            ));
            assert_eq!(
                state.tables.type_of(instantiated).alias_symbol,
                Some(box_symbol)
            );
            let w = annotation_of(state, "w");
            let again = state.get_type_from_type_node(w).expect("Box<string>");
            assert_eq!(again, instantiated, "instantiation interning");
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}
