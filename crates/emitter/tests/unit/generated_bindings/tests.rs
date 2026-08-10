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
    assert!(scopes.exit(outer_scope, inner).is_empty());
    assert!(scopes.exit(source, outer).is_empty());
}
