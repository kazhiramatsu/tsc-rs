use tsc_binder::bind_source_file;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions};
use tsc_types::{
    CompilerOptions, ElementFlags, IntersectionState, RelationComparisonResult, TupleTargetFlags,
};

use super::{RelationCaches, RelationKind};
use crate::state::CheckerState;

fn with_state<R>(run: impl FnOnce(&mut CheckerState) -> R) -> R {
    let options = CompilerOptions::default();
    let source = parse_source_file(
        "relate-test.ts".to_owned(),
        String::new(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    run(&mut state)
}

#[test]
fn relation_caches_are_per_relation() {
    let mut caches = RelationCaches::default();
    caches
        .cache_mut(RelationKind::Assignable)
        .insert("1,2".to_owned(), RelationComparisonResult::SUCCEEDED);
    assert!(caches.cache(RelationKind::Assignable).contains_key("1,2"));
    for relation in RelationKind::ALL {
        if relation != RelationKind::Assignable {
            assert!(
                !caches.cache(relation).contains_key("1,2"),
                "{relation:?} must not share the assignable cache"
            );
        }
    }
}

#[test]
fn relation_keys_swap_ids_for_identity_only() {
    with_state(|state| {
        let string = state.tables.intrinsics.string;
        let number = state.tables.intrinsics.number;
        let (small, large) = if string.0 < number.0 {
            (string, number)
        } else {
            (number, string)
        };
        let identity = state
            .get_relation_key(
                large,
                small,
                IntersectionState::NONE,
                RelationKind::Identity,
                false,
            )
            .expect("relation key");
        assert_eq!(identity, format!("{},{}", small.0, large.0));
        let assignable = state
            .get_relation_key(
                large,
                small,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            )
            .expect("relation key");
        assert_eq!(assignable, format!("{},{}", large.0, small.0));
        let suffixed = state
            .get_relation_key(
                small,
                large,
                IntersectionState::TARGET,
                RelationKind::Assignable,
                false,
            )
            .expect("relation key");
        assert_eq!(suffixed, format!("{},{}:2", small.0, large.0));
    });
}

#[test]
fn generic_reference_keys_use_backrefs() {
    with_state(|state| {
        // A tuple TARGET is a self-reference whose arguments are
        // its synthesized (unconstrained) type parameters — the
        // one M3-constructible generic-reference shape.
        let target = state.tables.get_tuple_target_type(
            TupleTargetFlags::new(&[ElementFlags::REQUIRED, ElementFlags::OPTIONAL])
                .expect("required/optional tuple is not single-rest"),
            false,
            None,
        );
        let key = state
            .get_relation_key(
                target,
                target,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            )
            .expect("relation key");
        // Shared type-parameter indices across both sides.
        assert_eq!(key, format!("{}=0=1,{}=0=1", target.0, target.0));
        // A concrete tuple reference is NOT a generic reference:
        // plain id-pair key.
        let number = state.tables.intrinsics.number;
        let string = state.tables.intrinsics.string;
        let concrete = state
            .create_normalized_type_reference_forced(target, &[number, string])
            .expect("tuple reference");
        let key = state
            .get_relation_key(
                concrete,
                concrete,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            )
            .expect("relation key");
        assert_eq!(key, format!("{},{}", concrete.0, concrete.0));
    });
}

#[test]
fn enum_relation_short_circuits_on_symbol_identity() {
    // 64676-64678: identical symbols relate before any flag or
    // name test — even a symbol that is not an enum at all.
    with_state(|state| {
        let symbol = tsc_binder::SymbolId(0);
        assert!(state
            .is_enum_type_related_to(symbol, symbol)
            .expect("identity path never escapes"));
    });
}

#[test]
fn constrained_type_parameters_do_not_share_backref_keys() {
    // m4-review A1: isUnconstrainedTypeParameter must FORCE
    // getConstraintOfTypeParameter (declared parameters keep the
    // constraint in the links slot). An inline-only read keys BOTH
    // generic-reference pairs below as `{Box}=0,{Box}=1`, so
    // `good`'s cached success swallows `bad`'s 2322 (tsc-probed:
    // 2322@151+3 on `bad` only, vendored 6.0.3 noLib).
    crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { value: T }\nfunction pair<T extends string, V extends T, U extends number>(a: Box<V>, b: Box<U>) {\n  const good: Box<T> = a;\n  const bad: Box<T> = b;\n  void good;\n  void bad;\n}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let rows: Vec<(u32, u32, u32)> = state
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
                .collect();
            assert_eq!(rows, [(2322, 151, 3)]);
        },
    );
}
