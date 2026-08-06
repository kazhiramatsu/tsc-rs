use tsc_syntax::NodeId;
use tsc_types::{CompilerOptions, ObjectFlags, TypeData, TypeFlags, TypeId};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_type(state: &mut CheckerState, name: &str) -> (NodeId, TypeId) {
    let annotation = find_probe_annotation(state.binder.source(0), name)
        .expect("declared variable has an annotation");
    let ty = state
        .get_type_from_type_node(annotation)
        .expect("mapped annotation resolves");
    (annotation, ty)
}

#[test]
fn mapped_type_model_constructibility() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() {\n\
               let v: { [K in keyof T]: T[K] };\n\
               let w: { readonly [P in keyof T]?: P };\n\
               let x: { -readonly [Q in keyof T as `x${Q & string}`]-?: T[Q] };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let (declaration, mapped) = annotation_type(state, "v");
            assert!(state.tables.flags_of(mapped).intersects(TypeFlags::OBJECT));
            assert!(state
                .tables
                .object_flags_of(mapped)
                .intersects(ObjectFlags::MAPPED));
            let TypeData::Mapped(data) = &state.tables.type_of(mapped).data else {
                panic!("mapped object flags require semantic mapped payload");
            };
            assert_eq!(data.declaration, declaration.0);
            assert_eq!(data.target, None);
            assert_eq!(data.mapper, None);
            assert_eq!(
                state
                    .get_type_from_type_node(declaration)
                    .expect("node cache requery"),
                mapped
            );
            let constraint = state
                .get_constraint_type_from_mapped_type(mapped)
                .expect("mapped constraint");
            assert!(matches!(
                state.tables.type_of(constraint).data,
                TypeData::Index { .. }
            ));
            assert_eq!(
                state
                    .type_to_string_slice(mapped)
                    .expect("every constructible mapped type renders"),
                "{ [K in keyof T]: T[K]; }"
            );

            let (_, optional) = annotation_type(state, "w");
            assert_eq!(
                state
                    .type_to_string_slice(optional)
                    .expect("mapped modifiers render"),
                "{ readonly [P in keyof T]?: P | undefined; }"
            );

            let (_, remapped) = annotation_type(state, "x");
            assert_eq!(
                state
                    .type_to_string_slice(remapped)
                    .expect("mapped key remap and subtractive modifiers render"),
                "{ -readonly [Q in keyof T as `x${Q & string}`]-?: T[Q]; }"
            );
        },
    );
}

#[test]
fn keyof_generic_remapped_type_stays_deferred() {
    with_program_state(
        &[(
            "a.ts",
            "function f<K extends string>() {\n\
               let keys: keyof { [P in K as `_${P}`]: P };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let (_, keys) = annotation_type(state, "keys");
            let TypeData::Index { ty: mapped, .. } = state.tables.type_of(keys).data else {
                panic!("keyof a generic remapped type stays an Index type");
            };
            assert!(state
                .tables
                .object_flags_of(mapped)
                .intersects(ObjectFlags::MAPPED));
            assert_eq!(
                state
                    .type_to_string_slice(keys)
                    .expect("deferred mapped keyof renders"),
                "keyof { [P in K as `_${P}`]: P; }"
            );
        },
    );
}

#[test]
fn mapped_type_modifiers_participate_in_identity() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() {\n\
               let a: { [P in keyof T]: T[P] };\n\
               let b: { [P in keyof T]?: T[P] };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let (_, plain) = annotation_type(state, "a");
            let (_, optional) = annotation_type(state, "b");
            assert!(!state
                .is_type_identical_to(plain, optional)
                .expect("mapped modifiers participate in identity"));
        },
    );
}
