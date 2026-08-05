use tsc_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;

#[test]
fn generic_interface_declared_type_is_a_generic_type_target() {
    with_program_state(
        &[("a.ts", "interface I<T> { a: T }\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("I", SymbolFlags::INTERFACE)
                .expect("I resolves");
            let declared = state
                .get_declared_type_of_class_or_interface(symbol)
                .expect("declared type in slice");
            let TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                this_type,
            } = state.tables.type_of(declared).data.clone()
            else {
                panic!("generic interfaces declare GenericType targets");
            };
            assert_eq!(type_parameters.len(), 1);
            assert_eq!(outer_type_parameter_count, 0);
            assert!(state
                .tables
                .object_flags_of(declared)
                .intersects(ObjectFlags::REFERENCE));
            assert!(matches!(
                state.tables.type_of(this_type).data,
                TypeData::TypeParameter {
                    is_this_type: true,
                    constraint: Some(constraint),
                } if constraint == declared
            ));
            // The instantiations map is seeded with the target:
            // referencing it with its own parameters IS the target.
            let reference = state
                .tables
                .create_type_reference(declared, &type_parameters);
            assert_eq!(reference, declared);
            assert!(state.could_contain_type_variables(declared));
        },
    );
}

#[test]
fn thisful_interface_declares_a_generic_type_without_parameters() {
    with_program_state(
        &[("a.ts", "interface I { m(): this }\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("I", SymbolFlags::INTERFACE)
                .expect("I resolves");
            let declared = state
                .get_declared_type_of_class_or_interface(symbol)
                .expect("declared type in slice");
            assert!(matches!(
                state.tables.type_of(declared).data,
                TypeData::GenericType {
                    ref type_parameters,
                    ..
                } if type_parameters.is_empty()
            ));
            assert!(
                !state.could_contain_type_variables(declared),
                "no type arguments to contain variables"
            );
        },
    );
}

#[test]
fn thisless_heritage_interface_stays_plain_but_members_escape() {
    with_program_state(
        &[(
            "a.ts",
            "interface A { a: string }\ninterface B extends A { b: string }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("B", SymbolFlags::INTERFACE)
                .expect("B resolves");
            let declared = state
                .get_declared_type_of_class_or_interface(symbol)
                .expect("declared type in slice");
            assert!(
                matches!(state.tables.type_of(declared).data, TypeData::Object),
                "thisless heritage interfaces stay plain InterfaceTypes"
            );
            // 5.3a: heritage members merge through getBaseTypes —
            // B sees its own `b` plus the inherited `a`.
            let members = state
                .resolve_structured_type_members(declared)
                .expect("heritage members resolve");
            let names: Vec<String> = state
                .members_of(members)
                .properties
                .iter()
                .map(|&p| state.binder.symbol(p).escaped_name.clone())
                .collect();
            assert_eq!(names, ["b", "a"], "own members first, inherited appended");
        },
    );
}

#[test]
fn cyclic_heritage_reads_the_thisless_shell() {
    with_program_state(
        &[(
            "a.ts",
            "interface A extends B { }\ninterface B extends A { }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let a = state
                .resolve_file_scope_name("A", SymbolFlags::INTERFACE)
                .expect("A resolves");
            let b = state
                .resolve_file_scope_name("B", SymbolFlags::INTERFACE)
                .expect("B resolves");
            let declared_a = state
                .get_declared_type_of_class_or_interface(a)
                .expect("A declared");
            let declared_b = state
                .get_declared_type_of_class_or_interface(b)
                .expect("B declared");
            // tsc's eagerly written shells observe "no thisType yet"
            // mid-cycle: both stay thisless.
            assert!(matches!(
                state.tables.type_of(declared_a).data,
                TypeData::Object
            ));
            assert!(matches!(
                state.tables.type_of(declared_b).data,
                TypeData::Object
            ));
        },
    );
}

#[test]
fn bare_reference_to_generic_interface_reports_2314() {
    with_program_state(
        &[("a.ts", "interface I<T> { a: T }\ndeclare var v: I;\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("var with annotation");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("errorType flows");
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
                    "Generic type 'I<T>' requires 1 type argument(s).".to_owned()
                )]
            );
        },
    );
}

#[test]
fn class_declared_types_are_generic_type_targets() {
    with_program_state(
        &[("a.ts", "class C<T> { }\nclass D { }\n")],
        &CompilerOptions::default(),
        |state| {
            let c = state
                .resolve_file_scope_name("C", SymbolFlags::CLASS)
                .expect("C resolves");
            let d = state
                .resolve_file_scope_name("D", SymbolFlags::CLASS)
                .expect("D resolves");
            let declared_c = state
                .get_declared_type_of_class_or_interface(c)
                .expect("C declared");
            let declared_d = state
                .get_declared_type_of_class_or_interface(d)
                .expect("D declared");
            assert!(matches!(
                state.tables.type_of(declared_c).data,
                TypeData::GenericType { ref type_parameters, .. } if type_parameters.len() == 1
            ));
            assert!(state
                .tables
                .object_flags_of(declared_c)
                .intersects(ObjectFlags::CLASS | ObjectFlags::REFERENCE));
            // kind === Class forces the GenericType shape even with
            // no parameters (57387).
            assert!(matches!(
                state.tables.type_of(declared_d).data,
                TypeData::GenericType { ref type_parameters, .. } if type_parameters.is_empty()
            ));
            assert!(!state.could_contain_type_variables(declared_d));
            assert!(state
                .tables
                .flags_of(declared_c)
                .intersects(TypeFlags::OBJECT));
        },
    );
}
