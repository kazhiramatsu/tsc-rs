use std::collections::BTreeSet;

use tsc_syntax::parse_source_file;

use super::{
    allocate_ordinary_temp_name, AncestorBindingPolicy, GeneratedBindingScopes,
    OrdinaryTempNamePolicy, ParsedSourceIdentifierNames, TransformArena,
};

#[test]
fn traversal_temp_policy_uses_final_scope_cursor() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            false,
            OrdinaryTempNamePolicy::FinalizerTraversal,
        ),
        "_a",
    );
    assert_eq!(scopes.allocate_temp(), "_b");
}

#[test]
fn authoritative_temp_policy_retains_available_planned_spelling() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            false,
            OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
        ),
        "_d",
    );
    assert_eq!(scopes.allocate_temp(), "_a");
}

#[test]
fn authoritative_temp_policy_falls_back_on_collision() {
    let mut scopes = GeneratedBindingScopes::new(
        BTreeSet::from(["_d".to_owned()]),
        AncestorBindingPolicy::AllowShadow,
    );

    assert_eq!(
        allocate_ordinary_temp_name(
            &mut scopes,
            "_d".into(),
            true,
            OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
        ),
        "_a",
    );
}

#[test]
fn parsed_identifier_snapshot_retains_erased_file_level_collisions() {
    let parsed = parse_source_file(
        "file-level-collisions.ts",
        concat!(
            "type _default = number;\n",
            "interface _default_1 {}\n",
            "declare const _default_2: number;\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let names =
        ParsedSourceIdentifierNames::collect(&arena, source).expect("parsed identifier snapshot");

    assert_eq!(names.optimistic_candidate("_default"), "_default_3");
    assert_eq!(
        names.optimistic_candidate("_default"),
        "_default_3",
        "candidate lookup is immutable for independent FileLevel IDs",
    );
}
