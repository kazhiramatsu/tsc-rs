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
        let symbol = state
            .binder
            .create_symbol(tsc_types::SymbolFlags::NONE, "identity".to_owned());
        assert_ne!(symbol.0 & tsc_types::TRANSIENT_SYMBOL_BIT, 0);
        assert!(state
            .is_enum_type_related_to(symbol, symbol)
            .expect("identity path never escapes"));
    });
}

#[test]
fn reporting_enum_relations_replay_cached_failures_with_typed_details() {
    let text = concat!(
        "namespace Target { export enum E { a, b, c } }\n",
        "namespace Extra { export enum E { a, b, c, d } }\n",
        "namespace Different { export enum E { a, b, c = 3 } }\n",
        "declare let target: Target.E;\n",
        "declare let extra: Extra.E;\n",
        "declare let different: Different.E;\n",
        "target = extra;\n",
        "target = different;\n",
    );
    crate::state::test_support::with_program_state(
        &[("a.ts", text)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let chains = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| {
                    (
                        diagnostic.message.text.clone(),
                        diagnostic
                            .message
                            .next
                            .iter()
                            .map(|detail| detail.text.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                chains,
                [
                    (
                        "Type 'Extra.E' is not assignable to type 'Target.E'.".to_owned(),
                        vec!["Property 'd' is missing in type 'Target.E'.".to_owned()],
                    ),
                    (
                        "Type 'Different.E' is not assignable to type 'Target.E'.".to_owned(),
                        vec!["Each declaration of 'E.c' differs in its value, where '2' was expected but '3' was given.".to_owned()],
                    ),
                ]
            );
        },
    );
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

#[test]
fn relation_error_suggests_nearby_string_literal_union_member() {
    let text = "type T1 = \"string\" | \"number\" | \"boolean\";\n\
                type T2 = T1 & (\"number\" | \"boolean\");\n\
                type T3 = T1 & (\"string\" | \"boolean\");\n\
                const t1: T1 = \"strong\";\n\
                const t2: T2 = \"strong\";\n\
                const t3: T3 = \"strong\";\n";
    crate::state::test_support::with_program_state(
        &[("a.ts", text)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| (diagnostic.code(), diagnostic.message_text().to_owned()))
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics,
                [
                    (
                        2820,
                        "Type '\"strong\"' is not assignable to type 'T1'. Did you mean '\"string\"'?"
                            .to_owned(),
                    ),
                    (
                        2322,
                        "Type '\"strong\"' is not assignable to type '\"number\" | \"boolean\"'."
                            .to_owned(),
                    ),
                    (
                        2820,
                        "Type '\"strong\"' is not assignable to type '\"string\" | \"boolean\"'. Did you mean '\"string\"'?"
                            .to_owned(),
                    ),
                ]
            );
        },
    );
}

#[test]
fn reporting_relation_retains_nested_no_common_properties_leaf() {
    crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "declare let source: { c: string };\n\
             declare let target: { a?: string } & { b?: string };\n",
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            let source_node =
                crate::relpin::find_probe_annotation(state.binder.source(0), "source")
                    .expect("source annotation");
            let target_node =
                crate::relpin::find_probe_annotation(state.binder.source(0), "target")
                    .expect("target annotation");
            let source = state
                .get_type_from_type_node(source_node)
                .expect("source type");
            let target = state
                .get_type_from_type_node(target_node)
                .expect("target type");
            let output = state
                .relation_error_output_with_context(
                    source,
                    target,
                    RelationKind::Assignable,
                    None,
                    None,
                )
                .expect("relation succeeds")
                .expect("weak target is unrelated");

            assert_eq!(output.message.code, 2559);
            assert_eq!(
                output.message.text,
                "Type '{ c: string; }' has no properties in common with type '{ a?: string | undefined; } & { b?: string | undefined; }'."
            );
            assert!(output.message.next.is_empty());
        },
    );
}
