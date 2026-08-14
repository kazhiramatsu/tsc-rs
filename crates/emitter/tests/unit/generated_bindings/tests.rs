use std::collections::BTreeSet;

use super::{AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes};

#[test]
fn descendant_reserved_preferred_bindings_still_reuse_in_siblings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super",
    );
    let (first_scope, nested) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super_1",
    );
    let _ = scopes.exit(first_scope, nested);
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super",
    );
    let _ = scopes.exit(source, sibling);
}

#[test]
fn preferred_reconciliation_advances_from_the_planned_suffix() {
    let mut scopes = GeneratedBindingScopes::new(
        BTreeSet::from(["_super".to_owned(), "_super_1".to_owned()]),
        AncestorBindingPolicy::AllowShadow,
    );
    let (source, function) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super_1".into(), true),
        "_super_2",
    );
    let _ = scopes.exit(source, function);
}

#[test]
fn eager_local_preferred_reservations_are_not_hoisted_bindings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_local_preferred_with_policy("_super".into(), true),
        "_super",
    );
    let (outer_scope, inner) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_local_preferred_with_policy("_super".into(), true),
        "_super_1",
    );
    assert!(scopes.exit(outer_scope, inner).names().is_empty());
    assert!(scopes.exit(source, outer).names().is_empty());
}

#[test]
fn formatted_private_temps_have_a_role_local_sequence() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(scopes.allocate_local_temp(), "_a");
    assert_eq!(
        scopes.allocate_private_temp_with_role_suffix("_accessor_storage", &BTreeSet::new(),),
        "_a_accessor_storage",
    );
    assert_eq!(
        scopes.allocate_private_temp_with_role_suffix("_accessor_storage", &BTreeSet::new(),),
        "_b_accessor_storage",
    );
}

#[test]
fn generated_private_names_reserve_ancestors_but_reuse_in_siblings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "a",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "a_accessor_storage",
    );
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "a",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "a_1_accessor_storage",
    );
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "b",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "b_accessor_storage",
    );
    let _ = scopes.exit(source, outer);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "b",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "b_accessor_storage",
    );
    let _ = scopes.exit(source, sibling);
}
