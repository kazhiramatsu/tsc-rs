use tsc_types::{CompilerOptions, SymbolFlags, TypeData, TypeFlags, TypeId};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let node = find_probe_annotation(state.binder.source(0), name)
        .unwrap_or_else(|| panic!("annotation for {name}"));
    state
        .get_type_from_type_node(node)
        .unwrap_or_else(|err| panic!("{name} resolves: {err}"))
}

#[test]
fn no_infer_type_production() {
    with_program_state(
        &[(
            "a.ts",
            "type NoInfer<T> = intrinsic;\n\
             declare let primitive: NoInfer<string>;\n\
             declare let object: NoInfer<{ x: string }>;\n\
             function keys<T>() { let key: keyof NoInfer<T>; }\n\
             declare function choose<T extends string>(value: T, fallback: NoInfer<T>): T;\n\
             choose(\"foo\", \"bar\");\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let primitive_node =
                find_probe_annotation(state.binder.source(0), "primitive").expect("primitive");
            let primitive = state
                .get_type_from_type_node(primitive_node)
                .expect("primitive NoInfer erases");
            assert_eq!(primitive, state.tables.intrinsics.string);

            let object_node =
                find_probe_annotation(state.binder.source(0), "object").expect("object");
            let object = state
                .get_type_from_type_node(object_node)
                .expect("object NoInfer constructs");
            assert!(state.tables.is_no_infer_type(object));
            assert_eq!(
                state.type_to_string_slice(object).expect("NoInfer display"),
                "NoInfer<{ x: string; }>"
            );

            let key_node = find_probe_annotation(state.binder.source(0), "key").expect("key");
            let key = state
                .get_type_from_type_node(key_node)
                .expect("keyof NoInfer constructs");
            let TypeData::Substitution(key_data) = state.tables.type_of(key).data.clone() else {
                panic!("keyof NoInfer<T> preserves the inference barrier");
            };
            assert!(state.tables.is_no_infer_type(key));
            assert!(state
                .tables
                .flags_of(key_data.base_type)
                .intersects(TypeFlags::INDEX));

            state.check_source_file(0);
            assert!(state
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2345));

            let choose = state
                .resolve_file_scope_name("choose", SymbolFlags::FUNCTION)
                .expect("choose resolves");
            assert!(state.get_type_of_symbol(choose).is_ok());
        },
    );
}

#[test]
fn conditional_resolution_distribution_inference_and_simplification() {
    with_program_state(
        &[(
            "a.ts",
            "type Select<T> = T extends string ? T : never;\n\
             type Identity<T> = T extends infer U ? U : never;\n\
             declare let falseBranch: number extends string ? 1 : 2;\n\
             declare let inferred: string extends infer U ? U : never;\n\
             declare let distributed: Select<\"a\" | 1>;\n\
             declare let expectedDistributed: \"a\";\n\
             declare let identity: Identity<\"x\" | 2>;\n\
             declare let expectedIdentity: \"x\" | 2;\n\
             function deferred<T>() {\n\
               let branch: T extends string ? 1 : 2;\n\
               let expectedDefault: 1 | 2;\n\
               let same: T extends unknown ? T : never;\n\
               let expectedSame: T;\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            assert_eq!(
                annotation_type(state, "falseBranch"),
                state.tables.get_number_literal_type(2.0),
            );
            assert_eq!(
                annotation_type(state, "inferred"),
                state.tables.intrinsics.string,
            );
            assert_eq!(
                annotation_type(state, "distributed"),
                annotation_type(state, "expectedDistributed"),
            );
            assert_eq!(
                annotation_type(state, "identity"),
                annotation_type(state, "expectedIdentity"),
            );

            let branch = annotation_type(state, "branch");
            assert!(state
                .tables
                .flags_of(branch)
                .intersects(TypeFlags::CONDITIONAL));
            let default_constraint = state
                .get_default_constraint_of_conditional_type(branch)
                .expect("default conditional constraint");
            assert_eq!(
                default_constraint,
                annotation_type(state, "expectedDefault"),
            );

            let same = annotation_type(state, "same");
            let simplified = state
                .get_simplified_type(same, /*writing*/ false)
                .expect("conditional simplification");
            assert_eq!(simplified, annotation_type(state, "expectedSame"));
        },
    );
}

#[test]
fn conditional_checker_consumers_do_not_fabricate_constraints_or_cycles() {
    with_program_state(
        &[(
            "a.ts",
            "type PropertyKey = string | number | symbol;\n\
             type UnexpectedError<T extends PropertyKey> = T;\n\
             type Example<T, U> = {\n\
               [K in keyof T]: K extends keyof U ? UnexpectedError<K> : K\n\
             };\n\
             type StrictExtract<T, U> = T extends U ? U extends T ? T : never : never;\n\
             type StrictExclude<T, U> = T extends StrictExtract<T, U> ? never : T;\n\
             type A<T> = { [Q in { [P in keyof T]: P }[keyof T]]: T[Q] };\n\
             type B<T, V> = A<{ [Q in keyof T]: StrictExclude<B<T[Q], V>, {}> }>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect();
            let positions: Vec<(u32, Option<u32>, Option<u32>)> = state
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
                .collect();
            assert!(
                !codes.iter().any(|code| matches!(code, 2315 | 2344 | 2456)),
                "{positions:?}"
            );
        },
    );
}
