use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;

fn commonjs_options() -> CompilerOptions {
    CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    }
}

#[test]
fn commonjs_anonymous_members_use_the_module_augmentation_merge_target() {
    with_program_state(
        &[
            ("/a.js", "exports.x = 1;\nthis.y;\nthis.missing;\n"),
            (
                "/augment.ts",
                "export {};\ndeclare module \"./a\" { export const y: number; }\n",
            ),
        ],
        &commonjs_options(),
        |state| {
            let root = state.binder.source(0).root;
            let raw = state
                .binder
                .node_symbol(root)
                .expect("CommonJS source files have a module symbol");
            let merged = state.get_merged_symbol(raw);
            assert_ne!(
                raw, merged,
                "the fixture must exercise a cloned module-augmentation merge target"
            );

            let ty = state
                .get_type_of_symbol(raw)
                .expect("the CommonJS module symbol has a value type");
            let mut names = state
                .get_properties_of_object_type_owned(ty)
                .expect("anonymous module members resolve")
                .into_iter()
                .map(|symbol| state.symbol_display_name(symbol))
                .collect::<Vec<_>>();
            names.sort();

            assert!(names.iter().any(|name| name == "x"), "{names:?}");
            assert!(names.iter().any(|name| name == "y"), "{names:?}");
            assert!(
                names.iter().all(|name| name != "missing"),
                "an undeclared sibling must remain absent: {names:?}"
            );
        },
    );
}

#[test]
fn commonjs_anonymous_members_without_augmentation_keep_the_original_symbol() {
    with_program_state(
        &[("/a.js", "exports.x = 1;\n")],
        &commonjs_options(),
        |state| {
            let root = state.binder.source(0).root;
            let raw = state
                .binder
                .node_symbol(root)
                .expect("CommonJS source files have a module symbol");
            assert_eq!(
                state.get_merged_symbol(raw),
                raw,
                "the no-augmentation control must not acquire a merge target"
            );

            let ty = state
                .get_type_of_symbol(raw)
                .expect("the CommonJS module symbol has a value type");
            let names = state
                .get_properties_of_object_type_owned(ty)
                .expect("anonymous module members resolve")
                .into_iter()
                .map(|symbol| state.symbol_display_name(symbol))
                .collect::<Vec<_>>();

            assert!(names.iter().any(|name| name == "x"), "{names:?}");
            assert!(
                names.iter().all(|name| name != "y"),
                "the control must not gain the augmentation member: {names:?}"
            );
        },
    );
}
