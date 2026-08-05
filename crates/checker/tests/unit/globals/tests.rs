use tsc_types::{CompilerOptions, TypeFlags};

use crate::state::test_support::with_program_state;

#[test]
fn declared_global_interface_resolves_through_get_global_type() {
    with_program_state(
        &[("a.ts", "interface Foo { x: number }\n")],
        &CompilerOptions::default(),
        |state| {
            let resolved = state
                .get_global_type("Foo", 0, true)
                .expect("in slice")
                .expect("reportErrors");
            assert!(state
                .tables
                .flags_of(resolved)
                .intersects(TypeFlags::OBJECT));
            assert_ne!(resolved, state.empty_object_type);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn missing_global_array_reports_2318_once_and_falls_back() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let first = state.global_array_type().expect("in slice");
        assert_eq!(first, state.empty_generic_type);
        let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
        assert_eq!(codes, [2318]);
        assert_eq!(state.diagnostics[0].file_name, None);
        // Memoized: the second call re-reports nothing.
        let second = state.global_array_type().expect("in slice");
        assert_eq!(second, first);
        assert_eq!(state.diagnostics.len(), 1);
    });
}

#[test]
fn non_generic_global_array_reports_2317_arity_error() {
    with_program_state(
        &[("a.ts", "interface Array { length: number }\n")],
        &CompilerOptions::default(),
        |state| {
            let resolved = state.global_array_type().expect("in slice");
            assert_eq!(resolved, state.empty_generic_type);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2317]);
            // The arity error sits on the interface declaration.
            assert_eq!(state.diagnostics[0].file_name.as_deref(), Some("a.ts"));
        },
    );
}

#[test]
fn missing_arity_zero_global_falls_back_to_empty_object() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let resolved = state.global_object_type().expect("in slice");
        assert_eq!(resolved, state.empty_object_type);
        let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
        assert_eq!(codes, [2318]);
    });
}
