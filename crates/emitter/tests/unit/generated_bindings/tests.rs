use std::collections::BTreeSet;

use super::{AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes};

#[test]
fn planned_temp_is_retained_when_available() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_c".into(), false),
        "_c",
    );
}

#[test]
fn duplicate_planned_temp_in_same_scope_falls_back_to_temp_sequence() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_b",
    );
}

#[test]
fn planned_temp_can_be_reused_in_sibling_scopes() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    let _ = scopes.exit(source, sibling);
}

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
fn file_level_optimistic_peers_share_text_but_reserve_descendants() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(
        scopes.reserve_planned_file_level_optimistic_with_policy("_default".into(), true),
        "_default",
    );
    assert_eq!(
        scopes.reserve_planned_file_level_optimistic_with_policy("_default".into(), true),
        "_default",
    );

    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_default", "_default".into(), true),
        "_default_1",
    );
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_default", "_default".into(), true),
        "_default_1",
    );
    let _ = scopes.exit(source, sibling);
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

// ================================================================
// H2.5h-b B-1: the E-NAMES-H policy-arm contracts (packet §12.3(b))
// and the loop-variable / node-keyed completion surface.
// ================================================================

#[test]
fn loop_variable_prefers_the_dedicated_slot_once_per_scope() {
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, body) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    assert_eq!(scopes.allocate_loop_variable(false), "_a");
    assert_eq!(scopes.allocate_temp(), "_b");
    let _ = scopes.exit(source, body);
}

#[test]
fn occupied_loop_slot_falls_through_to_the_temp_sequence() {
    let mut scopes = GeneratedBindingScopes::new(
        ["_i".to_owned()].into_iter().collect(),
        AncestorBindingPolicy::Reserve,
    );
    assert_eq!(scopes.allocate_loop_variable(false), "_a");
}

#[test]
fn sibling_scopes_reuse_the_loop_variable_spelling() {
    // §12.3(b) sibling-reuse arm: tsc resets tempFlags per function, so
    // sibling function scopes may both own `_i`.
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let _ = scopes.exit(source, first);
    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let _ = scopes.exit(source, sibling);
}

#[test]
fn active_ancestor_bindings_stay_reserved_in_descendants() {
    // §12.3(b) ancestor-reservation arm: an active ancestor's generated
    // bindings remain reserved while a descendant scope allocates.
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_temp(), "_a");
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let (outer_id, inner) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_b");
    assert_eq!(scopes.allocate_temp(), "_c");
    let _ = scopes.exit(outer_id, inner);
    let _ = scopes.exit(source, outer);
}

#[test]
fn node_keyed_allocation_is_stable_per_node_and_advances_per_source_name() {
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 1), "loop_init"),
        "loop_init_1",
    );
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 1), "loop_init"),
        "loop_init_1",
    );
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 2), "loop_init"),
        "loop_init_2",
    );
}

#[test]
fn source_occupied_allocator_pushes_past_the_reserved_names() {
    // §12.3(a) universe-equality direction: parsed identifiers occupy the
    // allocator exactly as tsc's file-level unique-name predicate does.
    let mut scopes = GeneratedBindingScopes::new(
        ["_a", "_b", "_i", "_super"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        AncestorBindingPolicy::Reserve,
    );
    assert_eq!(scopes.allocate_temp(), "_c");
    assert_eq!(scopes.allocate_loop_variable(false), "_d");
}
