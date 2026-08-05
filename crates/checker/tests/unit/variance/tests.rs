use tsc_types::{CompilerOptions, RelationComparisonResult, VarianceFlags};

use crate::links::LinkSlot;
use crate::relate::RelationKind;
use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), run)
}

fn annotation_type(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let annotation =
        find_probe_annotation(state.binder.source(0), name).expect("declared var with annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("annotation resolves")
}

fn measured_variances(state: &CheckerState, name: &str) -> Vec<VarianceFlags> {
    let symbol = *state.globals.get(name).expect("global interface symbol");
    match &state.links.symbol(symbol).variances {
        LinkSlot::Resolved(list) => list.to_vec(),
        other => panic!("variances not measured for {name}: {other:?}"),
    }
}

#[test]
fn structural_measurement_covers_the_four_shapes() {
    with_state(
        "interface Out2<T> { x: T }\n\
         interface Contra<T> { f: (x: T) => void }\n\
         interface Inv<T> { f: (x: T) => T }\n\
         interface Empty<T> { }\n\
         declare var a: Out2<\"a\">;\ndeclare var b: Out2<string>;\n\
         declare var c: Contra<\"a\">;\ndeclare var d: Contra<string>;\n\
         declare var e: Inv<\"a\">;\ndeclare var f: Inv<string>;\n\
         declare var g: Empty<\"a\">;\ndeclare var h: Empty<string>;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            // Covariant: Out2<"a"> → Out2<string>, not back.
            assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
            assert_eq!(state.is_type_assignable_to(b, a), Ok(false));
            assert_eq!(
                measured_variances(state, "Out2"),
                vec![VarianceFlags::COVARIANT]
            );
            // Contravariant: Contra<string> → Contra<"a">, not back.
            let c = annotation_type(state, "c");
            let d = annotation_type(state, "d");
            assert_eq!(state.is_type_assignable_to(d, c), Ok(true));
            assert_eq!(state.is_type_assignable_to(c, d), Ok(false));
            assert_eq!(
                measured_variances(state, "Contra"),
                vec![VarianceFlags::CONTRAVARIANT]
            );
            // Invariant: neither direction.
            let e = annotation_type(state, "e");
            let f = annotation_type(state, "f");
            assert_eq!(state.is_type_assignable_to(e, f), Ok(false));
            assert_eq!(state.is_type_assignable_to(f, e), Ok(false));
            assert_eq!(
                measured_variances(state, "Inv"),
                vec![VarianceFlags::INVARIANT]
            );
            // Independent (67335-67337: bivariant probes promote):
            // unused parameters relate regardless of arguments.
            let g = annotation_type(state, "g");
            let h = annotation_type(state, "h");
            assert_eq!(state.is_type_assignable_to(g, h), Ok(true));
            assert_eq!(state.is_type_assignable_to(h, g), Ok(true));
            assert_eq!(
                measured_variances(state, "Empty"),
                vec![VarianceFlags::INDEPENDENT]
            );
        },
    );
}

#[test]
fn modifier_fast_path_skips_measurement() {
    with_state(
        "interface O<out T> { x: T }\ninterface I<in T> { f: (x: T) => void }\n\
         interface IO<in out T> { x: T }\n\
         declare var a: O<\"a\">;\ndeclare var b: O<string>;\n\
         declare var c: IO<\"a\">;\ndeclare var d: IO<string>;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
            assert_eq!(
                measured_variances(state, "O"),
                vec![VarianceFlags::COVARIANT]
            );
            // in out → Invariant without probes (67326-67328).
            let c = annotation_type(state, "c");
            let d = annotation_type(state, "d");
            assert_eq!(state.is_type_assignable_to(c, d), Ok(false));
            assert_eq!(state.is_type_assignable_to(d, c), Ok(false));
            assert_eq!(
                measured_variances(state, "IO"),
                vec![VarianceFlags::INVARIANT]
            );
            // `in T` never measured until something relates it —
            // the slot stays vacant, proving the fast path.
            let i_symbol = *state.globals.get("I").expect("interface I");
            assert!(matches!(
                state.links.symbol(i_symbol).variances,
                LinkSlot::Vacant
            ));
        },
    );
}

#[test]
fn alias_variances_drive_the_same_alias_fast_path() {
    with_state(
        "type Box<T> = { x: T };\n\
         declare var a: Box<\"a\">;\ndeclare var b: Box<string>;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
            assert_eq!(state.is_type_assignable_to(b, a), Ok(false));
            let box_symbol = *state.globals.get("Box").expect("alias Box");
            assert_eq!(
                match &state.links.symbol(box_symbol).variances {
                    LinkSlot::Resolved(list) => list.to_vec(),
                    other => panic!("alias variances unmeasured: {other:?}"),
                },
                vec![VarianceFlags::COVARIANT]
            );
        },
    );
}

#[test]
fn template_members_mark_unreliable_variance_and_cache_entries() {
    with_state(
        "interface Tmpl<T extends string> { x: `a${T}` }\n\
         declare var a: Tmpl<\"a\">;\ndeclare var b: Tmpl<string>;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            let _ = state.is_type_assignable_to(a, b);
            let variances = measured_variances(state, "Tmpl");
            assert_eq!(variances.len(), 1);
            assert!(
                variances[0].intersects(VarianceFlags::UNRELIABLE),
                "template-vs-template relations under measurement fire the \
                 unreliable marker (66279): {variances:?}"
            );
            // The measurement's inner relation writes persisted the
            // ReportsUnreliable bit into the assignable cache
            // (65853/65865) — the 5.3b format extension.
            assert!(
                state
                    .relations
                    .cache(RelationKind::Assignable)
                    .values()
                    .any(|entry| entry.intersects(RelationComparisonResult::REPORTS_UNRELIABLE)),
                "no cache entry carries ReportsUnreliable"
            );
        },
    );
}
