use tsc_syntax::NodeData;
use tsc_types::{
    CompilerOptions, ElementFlags, IndexFlags, ScriptTarget, SymbolFlags, TypeData, TypeFlags,
    TypeId,
};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let annotation =
        find_probe_annotation(state.binder.source(0), name).expect("fixture annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("mapped annotation resolves")
}

fn property(state: &mut CheckerState, ty: TypeId, name: &str) -> tsc_binder::SymbolId {
    state
        .get_property_of_type_full(ty, name)
        .expect("mapped members resolve")
        .expect("mapped property exists")
}

fn parameter_annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let annotation = {
        let source = state.binder.source(0);
        (0..source.arena.len())
            .find_map(|index| {
                let NodeData::Parameter(parameter) =
                    &source.arena.node(tsc_syntax::NodeId(index as u32)).data
                else {
                    return None;
                };
                let declared_name = parameter.name?;
                let NodeData::Identifier(identifier) = &source.arena.node(declared_name).data
                else {
                    return None;
                };
                (identifier.text == name)
                    .then_some(parameter.r#type)
                    .flatten()
            })
            .expect("fixture parameter annotation")
    };
    state
        .get_type_from_type_node(annotation)
        .expect("parameter annotation resolves")
}

#[test]
fn finite_mapped_members_remap_duplicate_keys_and_instantiate_values() {
    with_program_state(
        &[(
            "a.ts",
            "declare let finite: { [K in \"a\" | \"b\"]?: K };\n\
             declare let remapped: { [K in \"a\" | \"b\" as `x${K}`]-?: K };\n\
             declare let duplicate: { [K in \"a\" | \"b\" as \"x\"]: K };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let finite = annotation_type(state, "finite");
            assert!(!state
                .is_generic_mapped_type_state(finite)
                .expect("finite mapped classifier"));
            let names: Vec<_> = state
                .get_properties_of_type_full(finite)
                .expect("finite properties")
                .into_iter()
                .map(|symbol| state.binder.symbol(symbol).escaped_name.clone())
                .collect();
            assert_eq!(names, ["a", "b"]);
            let a = property(state, finite, "a");
            assert!(state.symbol_flags(a).intersects(SymbolFlags::OPTIONAL));
            let a_type = state.get_type_of_symbol(a).expect("mapped value types");
            let a_text = state.type_to_string_slice(a_type).expect("value renders");
            assert!(a_text.contains("\"a\""), "{a_text}");
            assert!(a_text.contains("undefined"), "{a_text}");

            let remapped = annotation_type(state, "remapped");
            let remapped_names: Vec<_> = state
                .get_properties_of_type_full(remapped)
                .expect("remapped properties")
                .into_iter()
                .map(|symbol| state.binder.symbol(symbol).escaped_name.clone())
                .collect();
            assert_eq!(remapped_names, ["xa", "xb"]);
            let xa = property(state, remapped, "xa");
            assert!(!state.symbol_flags(xa).intersects(SymbolFlags::OPTIONAL));
            let xa_type = state.get_type_of_symbol(xa).expect("xa type");
            assert_eq!(
                state.type_to_string_slice(xa_type).expect("xa renders"),
                "\"a\""
            );

            let duplicate = annotation_type(state, "duplicate");
            let x = property(state, duplicate, "x");
            let x_type = state.get_type_of_symbol(x).expect("duplicate value union");
            assert!(state.tables.flags_of(x_type).intersects(TypeFlags::UNION));
            let TypeData::Union { types, .. } = &state.tables.type_of(x_type).data else {
                panic!("duplicate key value is a union");
            };
            assert_eq!(types.len(), 2);
        },
    );
}

#[test]
fn mapped_members_copy_modifiers_create_index_info_and_report_keyof() {
    with_program_state(
        &[(
            "a.ts",
            "declare let copied: { [K in keyof { readonly a?: number; b: string }]-?: K };\n\
             declare let indexed: { readonly [K in string]: number };\n\
             declare let remapped: { [K in \"a\" | \"b\" as `x${K}`]: K };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let copied = annotation_type(state, "copied");
            let a = property(state, copied, "a");
            let b = property(state, copied, "b");
            assert!(state.is_readonly_symbol(a));
            assert!(!state.is_readonly_symbol(b));
            assert!(!state.symbol_flags(a).intersects(SymbolFlags::OPTIONAL));

            let indexed = annotation_type(state, "indexed");
            let infos = state
                .get_index_infos_of_type(indexed)
                .expect("mapped index info");
            assert_eq!(infos.len(), 1);
            assert_eq!(infos[0].key_type, state.tables.intrinsics.string);
            assert_eq!(infos[0].value_type, state.tables.intrinsics.number);
            assert!(infos[0].is_readonly);

            let remapped = annotation_type(state, "remapped");
            let keys = state
                .get_index_type(remapped, IndexFlags::NONE)
                .expect("keyof remapped mapped type");
            let key_text = state.type_to_string_slice(keys).expect("key union renders");
            assert!(key_text.contains("\"xa\""), "{key_text}");
            assert!(key_text.contains("\"xb\""), "{key_text}");
        },
    );
}

#[test]
fn homomorphic_mapped_instantiation_preserves_array_and_tuple_shapes() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { [n: number]: T }\n\
             interface ReadonlyArray<T> { readonly [n: number]: T }\n\
             type Identity<T> = { [K in keyof T]: T[K] };\n\
             type Mutable<T> = { -readonly [K in keyof T]: T[K] };\n\
             type RequiredTuple<T> = { [K in keyof T]-?: T[K] };\n\
             declare let tuple: Identity<readonly [number, string?]>;\n\
             declare let mutable: Mutable<readonly number[]>;\n\
             declare let required: RequiredTuple<[number, string?]>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let tuple = annotation_type(state, "tuple");
            assert!(state.tables.is_tuple_type(tuple));
            let tuple_target = state.tables.reference_target(tuple);
            let TypeData::TupleTarget(tuple_data) = state.tables.type_of(tuple_target).data.clone()
            else {
                panic!("tuple instantiation retains a tuple target");
            };
            assert!(tuple_data.readonly);
            assert!(tuple_data.element_flags[1].intersects(ElementFlags::OPTIONAL));
            let tuple_arguments = state.get_type_arguments(tuple).expect("tuple elements");
            assert_eq!(tuple_arguments[0], state.tables.intrinsics.number);

            let mutable = annotation_type(state, "mutable");
            let mutable_text = state
                .type_to_string_slice(mutable)
                .expect("mutable renders");
            assert!(
                state.is_array_type(mutable).expect("array predicate"),
                "{mutable_text}: {:?}",
                state.tables.type_of(mutable).data
            );
            assert!(!state
                .is_readonly_array_type(mutable)
                .expect("readonly predicate"));
            assert_eq!(
                state
                    .get_element_type_of_array_type(mutable)
                    .expect("array element"),
                Some(state.tables.intrinsics.number)
            );

            let required = annotation_type(state, "required");
            assert!(state.tables.is_tuple_type(required));
            let required_target = state.tables.reference_target(required);
            let TypeData::TupleTarget(required_data) =
                state.tables.type_of(required_target).data.clone()
            else {
                panic!("required mapped tuple retains a tuple target");
            };
            assert!(required_data.element_flags[1].intersects(ElementFlags::REQUIRED));
        },
    );
}

#[test]
fn apparent_homomorphic_mapped_type_uses_array_base_constraint() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { [n: number]: T }\n\
             interface ReadonlyArray<T> { readonly [n: number]: T }\n\
             function f<T extends readonly string[]>(value: { [K in keyof T]: number }) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let mapped = parameter_annotation_type(state, "value");
            assert!(state
                .is_generic_mapped_type_state(mapped)
                .expect("generic mapped classifier"));
            let apparent = state
                .get_apparent_type(mapped)
                .expect("mapped apparent type resolves");
            let apparent_text = state
                .type_to_string_slice(apparent)
                .expect("apparent renders");
            assert!(
                state
                    .is_readonly_array_type(apparent)
                    .expect("apparent readonly array"),
                "{apparent_text}: {:?}",
                state.tables.type_of(apparent).data
            );
            assert_eq!(
                state
                    .get_element_type_of_array_type(apparent)
                    .expect("apparent array element"),
                Some(state.tables.intrinsics.number)
            );
        },
    );
}

#[test]
fn generic_indexed_mapped_substitution_preserves_template_and_optionality() {
    with_program_state(
        &[(
            "a.ts",
            "function f<K extends \"a\" | \"b\">(\n\
               value: { [P in \"a\" | \"b\"]: P }[K],\n\
               optional: { [P in \"a\" | \"b\"]?: P }[K]\n\
             ) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let value = parameter_annotation_type(state, "value");
            let TypeData::IndexedAccess {
                object_type,
                index_type,
                ..
            } = state.tables.type_of(value).data
            else {
                panic!("generic mapped access remains deferred");
            };
            assert!(state
                .is_mapped_type_generic_indexed_access(value)
                .expect("mapped generic indexed classifier"));
            let substituted = state
                .substitute_indexed_mapped_type(object_type, index_type)
                .expect("mapped template substitutes");
            assert_eq!(
                state
                    .type_to_string_slice(substituted)
                    .expect("substitution renders"),
                "K"
            );
            assert_eq!(
                state
                    .get_constraint_of_indexed_access(value)
                    .expect("constraint resolves"),
                Some(substituted)
            );

            let optional = parameter_annotation_type(state, "optional");
            let TypeData::IndexedAccess {
                object_type,
                index_type,
                ..
            } = state.tables.type_of(optional).data
            else {
                panic!("optional generic mapped access remains deferred");
            };
            let substituted = state
                .substitute_indexed_mapped_type(object_type, index_type)
                .expect("optional mapped template substitutes");
            let rendered = state
                .type_to_string_slice(substituted)
                .expect("optional substitution renders");
            assert!(rendered.contains('K'), "{rendered}");
            assert!(rendered.contains("undefined"), "{rendered}");
        },
    );
}

#[test]
fn generic_mapped_relations_compare_constraint_template_and_optionality() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>(\n\
               required: { [P in keyof T]: T[P] },\n\
               same: { [Q in keyof T]: T[Q] },\n\
               optional: { [P in keyof T]?: T[P] },\n\
               empty: {}\n\
             ) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let required = parameter_annotation_type(state, "required");
            let same = parameter_annotation_type(state, "same");
            let optional = parameter_annotation_type(state, "optional");
            let empty = parameter_annotation_type(state, "empty");
            assert!(state
                .is_type_assignable_to(required, same)
                .expect("equivalent mapped relation"));
            assert!(state
                .is_type_assignable_to(required, optional)
                .expect("required maps to optional"));
            assert!(!state
                .is_type_assignable_to(optional, required)
                .expect("optional does not map to required"));
            assert!(state
                .is_type_assignable_to(empty, optional)
                .expect("empty object maps to a partial mapped target"));
        },
    );
}

#[test]
fn mapped_circularity_preserves_quoted_property_name() {
    with_program_state(
        &[(
            "a.ts",
            "type NonOptionalKeys<T> = { [P in keyof T]: undefined extends T[P] ? never : P }[keyof T];\n\
             type Child<T> = { [P in NonOptionalKeys<T>]: T[P] };\n\
             interface ListWidget { \"type\": \"list\"; \"each\": Child<ListWidget>; }\n\
             type ListChild = Child<ListWidget>;\n\
             declare let value: ListChild;\n\
             value.type;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2615)
                .expect("mapped circularity diagnostic");
            assert_eq!(
                diagnostic.message_text(),
                "Type of property '\"each\"' circularly references itself in mapped type '{ [P in keyof ListWidget]: undefined extends ListWidget[P] ? never : P; }'."
            );
        },
    );
}

#[test]
fn recursively_expanding_union_defers_generic_mapped_indexed_access() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "recursivelyExpandingUnionNoStackoverflow.ts",
            "type N<T, K extends string> = T | { [P in K]: N<T, K> }[K];\n\n\
             type M = N<number, \"M\">;\n",
        )],
        &options,
        |state| {
            state.check_source_file(0);
            let diagnostics: Vec<_> = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start,
                        diagnostic.length,
                        diagnostic.message_text(),
                    )
                })
                .collect();
            assert_eq!(
                diagnostics,
                [
                    (
                        2589,
                        Some(70),
                        Some(14),
                        "Type instantiation is excessively deep and possibly infinite.",
                    ),
                    (
                        2615,
                        Some(70),
                        Some(14),
                        "Type of property 'M' circularly references itself in mapped type '{ [P in \"M\"]: any; }'.",
                    ),
                ]
            );
            assert!(
                state.mapped_types_in_progress.is_empty(),
                "mapped shell frames must balance after recursive resolution"
            );
        },
    );
}
