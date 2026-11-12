use crate::state::test_support::with_program_state;
use tsc_types::{CompilerOptions, ObjectFlags, TypeFlags};

#[test]
fn instantiable_variable_containment_caches_the_tsgo_answer() {
    // Go checker.go:22677-22690 caches all StructuredOrInstantiable
    // results, not only ObjectFlagsType. Diagnostics are pinned to tsgo
    // aa8149273, work/fixtures/P6.ts, --noEmit --strict --pretty false.
    let text = "\
function generic<T extends string, U extends { x: T }, V>(
    a: T, b: keyof U, c: U[\"x\"], d: `pre${T}`,
    e: V extends string ? { x: V } : { y: V }
) {
    let probeA: T;
    let probeB: keyof U;
    let probeC: U[\"x\"];
    let probeD: `pre${T}`;
    let probeE: V extends string ? { x: V } : { y: V };
    const badA: never = a;
    const badB: never = b;
    const badC: never = c;
    const badD: never = d;
    const badE: never = e;
}
";
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        for name in ["probeA", "probeB", "probeC", "probeD", "probeE"] {
            let annotation = crate::relpin::find_probe_annotation(state.binder.source(0), name)
                .expect("generic variable annotation");
            let ty = state.get_type_from_type_node(annotation).unwrap();
            assert!(state
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::INSTANTIABLE));
            let created = state.tables.len();
            assert!(state.could_contain_type_variables(ty));
            assert!(state.tables.object_flags_of(ty).contains(
                ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED
                    | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES
            ));
            assert!(state.could_contain_type_variables(ty));
            assert_eq!(state.tables.len(), created);
        }
        let primitive = state.tables.intrinsics.string;
        let before = state.tables.object_flags_of(primitive);
        assert!(!state.could_contain_type_variables(primitive));
        assert_eq!(state.tables.object_flags_of(primitive), before);
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
            [
                "Type 'T' is not assignable to type 'never'.",
                "Type 'string | number | symbol' is not assignable to type 'never'.",
                "Type 'U[\"x\"]' is not assignable to type 'never'.",
                "Type '`pre${T}`' is not assignable to type 'never'.",
                "Type '{ x: V; } | { y: V; }' is not assignable to type 'never'.",
            ]
        );
    });
}
