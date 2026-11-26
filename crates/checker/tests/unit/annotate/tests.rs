use tsc_binder::bind_source_file;
use tsc_syntax::{parse_published_source_file, ParseOptions, ScriptKind, SourceFile};
use tsc_types::{
    CheckFlags, CompilerOptions, ElementFlags, ObjectFlags, SignatureFlags, TypeData, TypeFlags,
    TypeId,
};

use crate::links::LinkSlot;
use crate::member_table::HashedName;
use crate::relpin::find_probe_annotation;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

fn parse(text: &str) -> SourceFile {
    let source = parse_published_source_file(
        "annotate-test.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "test source must parse cleanly: {:?}",
        source.parse_diagnostics
    );
    source
}

fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let annotation =
        find_probe_annotation(state.binder.source(0), name).expect("declared var with annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("annotation resolves in the M3 slice")
}

fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
    let options = CompilerOptions::default();
    let mut source = parse(text);
    let binder = bind_source_file(&mut source);
    let mut state = CheckerState::new(binder.source, &binder, &options);
    run(&mut state)
}

#[test]
fn deferred_reference_members_classify_the_frozen_property_selection() {
    let source = "interface All { z: number; a: number; }
        interface Wide<A, B, C, D, E, F, G, H> { z: number; }
        interface Small<T> { (): T; z: T; }
        interface Large<T> { (): T; z: T; a: T; b: T; c: T; d: T; e: T; f: T; g: T; h: T; }";
    for stable in [false, true] {
        with_state(source, |state| {
            state.stable_type_ordering = stable;
            for name in ["All", "Small", "Large", "Wide"] {
                let mut owner = *state.globals.get(name).unwrap();
                if name == "All" {
                    owner = state.clone_symbol(owner);
                    // Present the properties in a different order from their
                    // declarations so stable ordering must rearrange them.
                    let members =
                        std::sync::Arc::make_mut(state.binder.symbol_mut(owner).members_mut());
                    let z = tsc_types::known_name!("z");
                    let symbol = members.shift_remove(z).unwrap();
                    members.insert(z, symbol);
                }
                let target = state.get_declared_type_of_symbol(owner).unwrap();
                let source = state.resolve_declared_members(target).unwrap();
                let record = state.members_of(source);
                let properties = record.properties.clone();
                let member_ids = record.members.symbol_list();
                let index = state.declared_property_indexes.get(&source);
                match name {
                    "Large" | "Wide" => {
                        assert!(index.unwrap().sorted.as_deref().unwrap().is_sorted())
                    }
                    "All" if stable => {
                        assert_ne!(&*member_ids, &*properties);
                        assert!(index.unwrap().sorted.is_none());
                    }
                    _ => assert!(index.is_none()),
                }
                for &id in &member_ids {
                    let key = state.binder.symbol(id).escaped_name;
                    assert_eq!(
                        state.declared_property(source, HashedName::new(key)),
                        properties.contains(&id).then_some(id)
                    );
                }
                if name == "All" && stable {
                    let extra_name = tsc_types::known_name!("extra");
                    let extra = state
                        .binder
                        .create_symbol(tsc_types::SymbolFlags::PROPERTY, extra_name);
                    let mut members = state.members_of(source).members.clone().into_builder();
                    members.insert(&state.binder, extra);
                    state.members_mut(source).members = members.into();
                    assert_eq!(
                        state.declared_property(source, HashedName::new(extra_name)),
                        None
                    );
                }
                // Replace just the property list under the published ID.
                state.members_mut(source).properties = Default::default();
                for &id in &member_ids {
                    assert_eq!(
                        state.declared_property(
                            source,
                            HashedName::new(state.binder.symbol(id).escaped_name)
                        ),
                        None
                    );
                }
            }
        });
    }
}

/// An instantiated type repeats its target's property order without a sort
/// of its own: a reference without base types lists its instantiated members
/// as the declared list has them (whatever the symbol's table order), and an
/// instantiated anonymous type as its target has them. With stable ordering
/// the debug build sorts each such list and asserts that nothing moves.
#[test]
fn instantiated_members_repeat_their_targets_property_order() {
    // `[key]` is declared first and bound late: the symbol's table has it
    // behind the early members, the declared list of stable ordering (by
    // where the declarations lie) ahead of them.
    let source = "declare const key: \"z\";
        interface Box<T> { [key]: T; a: T; m(): T; }
        declare var b: Box<string>;
        type Lit<T> = { z: T; a: T; q: T };
        declare var l: Lit<number>;
        declare var k: Lit<string>;";
    let mut declared_orders = Vec::new();
    for stable in [false, true] {
        let order = with_state(source, |state| {
            state.stable_type_ordering = stable;
            let names = |state: &CheckerState, properties: &[tsc_types::SymbolId]| -> Vec<_> {
                properties
                    .iter()
                    .map(|&id| state.binder.symbol(id).escaped_name)
                    .collect()
            };
            let owner = *state.globals.get("Box").unwrap();
            let target = state.get_declared_type_of_symbol(owner).unwrap();
            let declared_members = state.resolve_declared_members(target).unwrap();
            let declared = state.members_of(declared_members).properties.clone();
            let declared_names = names(state, &declared);
            assert_eq!(declared_names.len(), 3);
            if stable {
                assert_eq!(declared_names[0], tsc_types::known_name!("z"));
            }

            let reference = annotation_type(state, "b");
            assert_ne!(reference, target);
            let instantiated = state.get_properties_of_type(reference).unwrap();
            assert_eq!(names(state, &instantiated), declared_names);
            // Fresh members, not the declared ones.
            assert!(instantiated.iter().all(|id| !declared.contains(id)));

            let first = annotation_type(state, "l");
            let second = annotation_type(state, "k");
            assert_ne!(first, second);
            let expected = [
                tsc_types::known_name!("z"),
                tsc_types::known_name!("a"),
                tsc_types::known_name!("q"),
            ];
            for ty in [first, second] {
                let properties = state.get_properties_of_type(ty).unwrap();
                assert_eq!(names(state, &properties), expected);
            }
            declared_names
        });
        declared_orders.push(order);
    }
    // The premise: the table's order (what the list is without stable
    // ordering) and the order of the declarations are two orders here.
    assert_ne!(declared_orders[0], declared_orders[1]);
}

#[test]
fn a_deferred_reference_answers_the_weak_test_from_its_declarations() {
    with_state(
        "interface Optional<T> { first?: T; second?: T; }\n\
         interface Required<T> { first: T; second?: T; }\n\
         interface Empty<T> {}\n\
         declare const optional: Optional<string>;\n\
         declare const required: Required<string>;\n\
         declare const empty: Empty<string>;",
        |state| {
            for (name, expected) in [("optional", true), ("required", false), ("empty", false)] {
                let ty = annotation_type(state, name);
                assert_eq!(state.is_weak_type(ty).unwrap(), expected, "{name}");
                // No member was realized, nothing was resolved in full, and
                // the verdict is remembered.
                let deferred = state.lazy_members(ty).unwrap();
                assert!(deferred.instantiated.is_none(), "{name}");
                assert_eq!(deferred.weak, Some(expected), "{name}");
                assert!(
                    state
                        .links
                        .read_ty(ty, |links| links.resolved_members.resolved())
                        .is_none(),
                    "{name}"
                );
                assert_eq!(state.is_weak_type(ty).unwrap(), expected, "{name}");
            }
            // The members' identity and order afterwards are a full
            // resolution's, and the resolved record answers the same.
            let optional = annotation_type(state, "optional");
            let first = state
                .get_property_of_object_type(optional, "first")
                .unwrap()
                .unwrap();
            assert!(state.is_weak_type(optional).unwrap());
            let all = state.get_properties_of_type_full(optional).unwrap();
            assert_eq!(all[0], first);
            assert_eq!(state.binder.symbol(all[1]).escaped_name, "second");
            assert_eq!(
                state
                    .get_property_of_object_type(optional, "first")
                    .unwrap(),
                Some(first)
            );
            assert!(state.is_weak_type(optional).unwrap());
        },
    );
}

#[test]
fn the_weak_test_of_a_deferred_reference_follows_the_inherited_names() {
    with_state(
        "interface A<T> { value?: T; a?: T; }\n\
         interface B<T> { value: T; b?: T; }\n\
         interface AB<T> extends A<T>, B<T> { own?: T; }\n\
         interface BA<T> extends B<T>, A<T> { own?: T; }\n\
         interface Override<T> extends B<T> { value?: T; }\n\
         interface Deep<T> extends Override<T> { deep?: T; }\n\
         interface OnlyBase<T> extends A<T> {}\n\
         declare const ab: AB<string>; declare const ba: BA<string>;\n\
         declare const overridden: Override<string>; declare const deep: Deep<string>;\n\
         declare const only: OnlyBase<string>;",
        |state| {
            // An erroneous inheritance has the same table as a full
            // resolution builds - the first value holds a name - and the
            // verdict read from the declarations is that table's.
            for (name, expected) in [
                ("ab", true),
                ("ba", false),
                ("overridden", true),
                ("deep", true),
                ("only", true),
            ] {
                let ty = annotation_type(state, name);
                assert_eq!(state.is_weak_type(ty).unwrap(), expected, "{name}");
                assert!(state.lazy_members(ty).is_some(), "{name}");
                let properties = state.get_properties_of_type_full(ty).unwrap();
                assert_eq!(
                    !properties.is_empty()
                        && properties.iter().all(|&property| state
                            .symbol_flags(property)
                            .intersects(tsc_types::SymbolFlags::OPTIONAL)),
                    expected,
                    "{name}"
                );
            }
        },
    );
}

#[test]
fn the_weak_test_of_a_deferred_reference_creates_what_a_full_resolution_creates() {
    let run = |eager: bool, stable: bool| {
        with_state(
            "type Parent<T> = { left?: T; same?: T } &\n\
                              { right?: { wrapped: T }; same?: number };\n\
             interface Child<T> extends Parent<T> { required: T; }\n\
             declare const child: Child<string>;",
            |state| {
                state.stable_type_ordering = stable;
                let ty = annotation_type(state, "child");
                if eager {
                    state.structured_member_headers(ty).unwrap();
                    state.resolve_structured_type_members(ty).unwrap();
                } else {
                    assert!(!state.is_weak_type(ty).unwrap());
                }
                let boundary = (state.tables.len(), state.total_instantiation_count);
                let mut observed = Vec::new();
                for name in ["right", "left", "same", "required"] {
                    let property = state
                        .get_property_of_object_type(ty, name)
                        .unwrap()
                        .unwrap();
                    observed.push(state.get_type_of_symbol(property).unwrap());
                }
                (
                    boundary,
                    observed,
                    state.tables.len(),
                    state.total_instantiation_count,
                )
            },
        )
    };
    for stable in [false, true] {
        assert_eq!(run(false, stable), run(true, stable));
    }
}

#[test]
fn the_weak_test_settles_at_a_required_value_and_still_visits_every_base() {
    // A value that is not optional settles "not weak" where it is met - an
    // own property, or the second base's - and no further name is held. What
    // is left of the hierarchy (a chain of generic interfaces, an intersected
    // alias) is still visited as a full resolution visits it: the same types
    // and the same instantiation count exist afterwards.
    let run = |eager: bool| {
        with_state(
            "interface Root<T> { root?: T[]; }\n\
             interface Middle<T> extends Root<{ boxed: T }> { middle?: T; }\n\
             type Mixed<T> = { left?: T } & { right?: { wrapped: T } };\n\
             interface Leaf<T> extends Middle<T>, Mixed<T> { required: T; optional?: T; }\n\
             interface LateOptional<T> extends Middle<T> { late?: T; }\n\
             interface RequiredBase<T> { needed: T; }\n\
             interface FromBase<T> extends LateOptional<T>, RequiredBase<T> {}\n\
             declare const leaf: Leaf<string>; declare const from_base: FromBase<string>;",
            |state| {
                let mut boundaries = Vec::new();
                for name in ["leaf", "from_base"] {
                    let ty = annotation_type(state, name);
                    if eager {
                        state.structured_member_headers(ty).unwrap();
                        state.resolve_structured_type_members(ty).unwrap();
                    } else {
                        assert!(!state.is_weak_type(ty).unwrap(), "{name}");
                        assert!(state.lazy_members(ty).is_some(), "{name}");
                        // The verdict is remembered, and the second answer is the first.
                        assert!(!state.is_weak_type(ty).unwrap(), "{name}");
                    }
                    boundaries.push((state.tables.len(), state.total_instantiation_count));
                }
                boundaries
            },
        )
    };
    assert_eq!(run(false), run(true));
}

#[test]
fn the_weak_test_resolves_a_reference_that_is_being_finished() {
    with_state(
        "interface Box<T> { value?: T; }\ndeclare const box: Box<string>;",
        |state| {
            let ty = annotation_type(state, "box");
            state.structured_member_headers(ty).unwrap();
            state.lazy_members_mut(ty).unwrap().finishing = true;
            assert!(state.is_weak_type(ty).unwrap());
            assert!(state
                .links
                .read_ty(ty, |links| links.resolved_members.resolved())
                .is_some());
        },
    );
}

#[test]
fn a_lazy_tables_index_lives_in_its_types_slot_and_a_finished_tables_place_is_reused() {
    with_state(
        "interface Box<T> { value: T; plain: number }
         declare let a: Box<string>;
         declare let b: Box<number>;
         declare let c: Box<boolean>;",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            let c = annotation_type(state, "c");
            assert_eq!(state.lazy_members_index(a), None);
            state.structured_member_headers(a).unwrap();
            state.structured_member_headers(b).unwrap();
            let first = state.lazy_members_index(a).expect("a's table is ready");
            let second = state.lazy_members_index(b).expect("b's table is ready");
            assert_ne!(first, second);
            assert_eq!(state.deferred_reference_members.len(), 2);
            // The slot says "not resolved" to every reader of the members,
            // and the public reading says the type's members are resolved
            // (tsgo's flag), as before.
            assert_eq!(state.links.ty(a).resolved_members.resolved(), None);
            assert!(state.members_resolved(a));
            // A property read finds the table again and keeps its place.
            assert!(state
                .get_property_of_object_type(a, "value")
                .unwrap()
                .is_some());
            assert_eq!(state.lazy_members_index(a), Some(first));
            // The full members take the slot and free the table's place.
            let members = state.resolve_structured_type_members(a).unwrap();
            assert_eq!(state.links.ty(a).resolved_members.resolved(), Some(members));
            assert_eq!(state.lazy_members_index(a), None);
            assert_eq!(state.deferred_reference_members.len(), 1);
            // The next lazy table takes the freed place; b keeps its own.
            state.structured_member_headers(c).unwrap();
            assert_eq!(state.lazy_members_index(c), Some(first));
            assert_eq!(state.lazy_members_index(b), Some(second));
            assert_eq!(state.deferred_reference_members.len(), 2);
        },
    );
}

#[test]
fn deferred_reference_members_keep_identity_across_lookup_and_enumeration() {
    with_state(
        "interface Box<T> { value: T; other: T; }\n\
         declare const strings: Box<string>; declare const numbers: Box<number>;",
        |state| {
            let strings = annotation_type(state, "strings");
            let numbers = annotation_type(state, "numbers");
            let value = state
                .get_property_of_object_type(strings, "value")
                .unwrap()
                .unwrap();
            assert_eq!(
                state.get_type_of_symbol(value).unwrap(),
                state.tables.intrinsics.string
            );
            let all = state.get_properties_of_type_full(strings).unwrap();
            assert_eq!(all.len(), 2);
            assert!(all.contains(&value));
            assert_eq!(
                state.get_property_of_object_type(strings, "value").unwrap(),
                Some(value)
            );
            let other_value = state
                .get_property_of_object_type(numbers, "value")
                .unwrap()
                .unwrap();
            assert_ne!(value, other_value);
            assert_eq!(
                state.get_type_of_symbol(other_value).unwrap(),
                state.tables.intrinsics.number
            );
        },
    );
}

#[test]
fn deferred_reference_members_publish_each_frozen_identity_once() {
    use super::DeferredDeclaredMember;

    with_state(
        "interface Box<T> { plain: number; value: T } declare const box: Box<string>;",
        |state| {
            let ty = annotation_type(state, "box");
            let target = state.tables.reference_target(ty);
            let source = state.resolve_declared_members(target).unwrap();
            let table = &state.members_of(source).members;
            let plain = table.get(&state.binder, "plain").unwrap();
            let value = table.get(&state.binder, "value").unwrap();
            state.get_type_of_symbol(plain).unwrap();
            state.structured_member_headers(ty).unwrap();
            for (declared, unaffected) in [(plain, true), (value, false)] {
                let key = HashedName::new(state.binder.symbol(declared).escaped_name);
                let ready = state.lazy_members(ty).unwrap();
                assert!(
                    matches!(ready.declared_member(&state.binder, declared, key),
                DeferredDeclaredMember::Publish { declared: id, mapper } if id == declared && mapper.is_none() == unaffected)
                );
                let symbol = state
                    .deferred_reference_property(ty, key)
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(symbol == declared, unaffected);
                let ready = state.lazy_members(ty).unwrap();
                assert!(
                    matches!(ready.declared_member(&state.binder, declared, key), DeferredDeclaredMember::Cached(id) if id == symbol)
                );
                assert_eq!(
                    state.deferred_reference_property(ty, key).unwrap(),
                    Some(Some(symbol))
                );
            }
        },
    );
}

#[test]
fn deferred_reference_members_freeze_identity_before_declaration_type_warms() {
    with_state(
        "interface Box<T> { plain: number; value: T; }\ndeclare const box: Box<string>;",
        |state| {
            let box_type = annotation_type(state, "box");
            let target = state.tables.reference_target(box_type);
            let declared = state.resolve_declared_members(target).unwrap();
            let plain = state
                .members_of(declared)
                .members
                .get(&state.binder, "plain")
                .unwrap();
            assert!(state
                .links
                .symbol(plain)
                .type_of_symbol
                .resolved()
                .is_none());
            state.structured_member_headers(box_type).unwrap();
            assert!(
                state.members_resolved(box_type),
                "public flags retain the eager resolution boundary"
            );
            assert!(state.lazy_members(box_type).is_some());
            assert_eq!(
                state.get_type_of_symbol(plain).unwrap(),
                state.tables.intrinsics.number
            );
            let instantiated = state
                .get_property_of_object_type(box_type, "plain")
                .unwrap()
                .unwrap();
            assert_ne!(
                instantiated, plain,
                "a later cache hit cannot change the earlier identity decision"
            );
            assert_eq!(
                state.get_type_of_symbol(instantiated).unwrap(),
                state.tables.intrinsics.number
            );
            assert!(state
                .get_properties_of_type_full(box_type)
                .unwrap()
                .contains(&instantiated));
        },
    );
}

#[test]
fn deferred_reference_members_preserve_inherited_types_and_this() {
    with_state(
        "interface Base<T> { value: T; clone(): this; }\n\
         interface Derived<T> extends Base<T> { own: T; }\n\
         declare const item: Derived<string>;",
        |state| {
            let item = annotation_type(state, "item");
            let value = state
                .get_property_of_object_type(item, "value")
                .unwrap()
                .unwrap();
            assert_eq!(
                state.get_type_of_symbol(value).unwrap(),
                state.tables.intrinsics.string
            );
            let clone = state
                .get_property_of_object_type(item, "clone")
                .unwrap()
                .unwrap();
            let clone_type = state.get_type_of_symbol(clone).unwrap();
            let signatures = state
                .get_signatures_of_type(clone_type, crate::structural::SignatureKind::Call)
                .unwrap();
            assert_eq!(signatures.len(), 1);
            assert_eq!(
                state.get_return_type_of_signature(signatures[0]).unwrap(),
                item
            );
            let all = state.get_properties_of_type_full(item).unwrap();
            assert_eq!(all.len(), 3);
            assert!(all.contains(&value));
            assert!(all.contains(&clone));
        },
    );
}

#[test]
fn any_base_index_identity_preserves_semantic_indexing_and_distinguishes_declared_indexes() {
    with_state(
        "declare const Base: any; class Derived extends Base {}\n\
         declare const instance: Derived; declare const constructor: typeof Derived;\n\
         declare const ordinary: { [key: string]: any };",
        |state| {
            for name in ["instance", "constructor", "ordinary"] {
                let ty = annotation_type(state, name);
                let infos = state
                    .get_index_infos_of_type(ty)
                    .expect("resolved index infos");
                assert_eq!(infos.len(), 1, "{name}");
                assert_eq!(infos[0].key_type, state.tables.intrinsics.string);
                assert_eq!(infos[0].value_type, state.tables.intrinsics.any);
                assert_eq!(infos[0].is_any_base_type_index_info, name != "ordinary");
            }
        },
    );
}

#[test]
fn negative_literal_annotations_reserve_the_positive_operand_identities() {
    with_state("declare const value: -7;", |state| {
        let negative = annotation_type(state, "value");
        let positive = state.tables.get_number_literal_type(7.0);
        let positive_fresh = state.tables.get_fresh_type_of_literal_type(positive);
        assert!(positive.index() < negative.index());
        assert!(positive_fresh.index() < negative.index());
    });
}

#[test]
fn checked_js_non_jsdoc_base_constructor_error_is_published() {
    let result = check_program(
        &[
            InputFile::new(
                "first.js".to_owned(),
                "class Drakkhen extends Dragon {}\n".to_owned(),
            ),
            InputFile::new(
                "second.ts".to_owned(),
                "function Dragon(numberEaten: number) { void numberEaten; }\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diag| (
                diag.code(),
                diag.file_name
                    .as_ref()
                    .map(|value| value.as_js().as_str().expect("scalar name observation"))
                    .unwrap_or_default(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2507, "first.js", 23, 6)]
    );
}

#[test]
fn base_constructor_type_argument_count_error_uses_expression_span() {
    let text = "interface Base<T, U> {}\n\
                interface BaseConstructor { new <T, U>(): Base<T, U>; }\n\
                declare function getBase(): BaseConstructor;\n\
                class D extends getBase() <string, string, string> {}\n";
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), text.to_owned())],
        &CompilerOptions {
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2508)
        .expect("base constructor type-argument count error");
    let expected_start = text
        .find("getBase() <")
        .expect("heritage expression")
        .try_into()
        .expect("test offset fits u32");
    assert_eq!(
        (diagnostic.start, diagnostic.length),
        (Some(expected_start), Some(9))
    );
    assert_eq!(
        diagnostic.message.text,
        "No base constructor has the specified number of type arguments."
    );
}

#[test]
fn union_annotation_reduces_string_literals_matched_by_templates() {
    with_state(
        "declare var a: \"abc\" | `a${string}`;\ndeclare var b: `a${string}`;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            // The annotation path routes through the checker union
            // twin, so removeStringLiteralsMatchedByTemplateLiterals
            // collapses the union to the template itself, like
            // getUnionTypeWorker (61547-61549) does.
            assert_eq!(a, b);
        },
    );
}

#[test]
fn distinct_type_literals_are_distinct_types() {
    with_state(
        "declare var a: { x: number };\ndeclare var b: { x: number };\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_ne!(a, b, "anonymous object types never intern structurally");
            // Re-reading the SAME node returns the cached type.
            assert_eq!(annotation_type(state, "a"), a);
        },
    );
}

#[test]
fn literal_type_nodes_yield_regular_literals() {
    with_state("declare var a: 1;\ndeclare var b: \"x\";\n", |state| {
        let one = annotation_type(state, "a");
        assert!(!state.tables.is_fresh_literal_type(one));
        assert_eq!(
            state.tables.get_regular_type_of_literal_type(None, one),
            one
        );
        let x = annotation_type(state, "b");
        assert!(state
            .tables
            .flags_of(x)
            .intersects(TypeFlags::STRING_LITERAL));
    });
}

#[test]
fn union_annotations_intern_by_member_set() {
    with_state("declare var a: 1 | 2;\ndeclare var b: 2 | 1;\n", |state| {
        let a = annotation_type(state, "a");
        let b = annotation_type(state, "b");
        assert_eq!(a, b);
        assert!(state.tables.flags_of(a).intersects(TypeFlags::UNION));
    });
}

#[test]
fn tuple_annotations_build_normalized_references() {
    with_state(
        "declare var a: [number, string?];\ndeclare var b: readonly [number];\ndeclare var c: [number, ...[string, boolean]];\ndeclare var d: [number, string, boolean];\n",
        |state| {
            let a = annotation_type(state, "a");
            assert!(state.tables.is_tuple_type(a));
            let target = state.tables.reference_target(a);
            let TypeData::TupleTarget(data) = &state.tables.type_of(target).data else {
                panic!("tuple reference targets a tuple target");
            };
            assert_eq!(
                state.tables.tuple_target(*data).element_flags.as_ref(),
                [ElementFlags::REQUIRED, ElementFlags::OPTIONAL]
            );
            assert_eq!(state.tables.tuple_target(*data).min_length, 1);
            assert!(!state.tables.tuple_target(*data).readonly);
            // Optional element type widened with undefined (strict).
            let args = state.tables.type_arguments(a).to_vec();
            assert!(state.tables.flags_of(args[1]).intersects(TypeFlags::UNION));

            let b = annotation_type(state, "b");
            let b_target = state.tables.reference_target(b);
            let TypeData::TupleTarget(data) = &state.tables.type_of(b_target).data else {
                panic!("tuple reference targets a tuple target");
            };
            assert!(state.tables.tuple_target(*data).readonly);

            // Variadic tuple spread normalizes to the flat tuple.
            assert_eq!(annotation_type(state, "c"), annotation_type(state, "d"));
        },
    );
}

#[test]
fn recursive_interfaces_resolve_declared_types_and_members() {
    with_state(
        "interface A { next: B }\ninterface B { next: A }\ndeclare var a: A;\ndeclare var b: B;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_ne!(a, b);
            assert!(state
                .tables
                .object_flags_of(a)
                .intersects(ObjectFlags::INTERFACE));
            let members = state
                .resolve_structured_type_members(a)
                .expect("interface members resolve");
            let members = state.members_of(members).clone();
            assert_eq!(members.properties.len(), 1);
            let next = members.properties[0];
            let next_type = state.get_type_of_symbol(next).expect("property type");
            assert_eq!(next_type, b, "A.next is B's declared type");
        },
    );
}

#[test]
fn method_members_get_anonymous_types_with_call_signatures() {
    with_state(
        "declare var a: { m(x: 1): void, p: (x: number) => void };\n",
        |state| {
            let a = annotation_type(state, "a");
            let members_id = state
                .resolve_structured_type_members(a)
                .expect("type literal members resolve");
            let members = state.members_of(members_id).clone();
            assert_eq!(members.properties.len(), 2);

            let method_type = state
                .get_type_of_symbol(members.properties[0])
                .expect("method type");
            let method_members_id = state
                .resolve_structured_type_members(method_type)
                .expect("method members resolve");
            let method_members = state.members_of(method_members_id).clone();
            assert_eq!(method_members.call_signatures().len(), 1);
            let signature = state
                .signature_of(method_members.call_signatures()[0])
                .clone();
            assert!(signature.is_from_method());
            assert!(signature.flags.contains(SignatureFlags::HAS_LITERAL_TYPES));
            assert_eq!(signature.min_argument_count(), 1);

            let property_type = state
                .get_type_of_symbol(members.properties[1])
                .expect("function property type");
            let property_members_id = state
                .resolve_structured_type_members(property_type)
                .expect("function type members resolve");
            let property_members = state.members_of(property_members_id).clone();
            assert_eq!(property_members.call_signatures().len(), 1);
            assert!(!state
                .signature_of(property_members.call_signatures()[0])
                .is_from_method());
        },
    );
}

#[test]
fn index_signatures_produce_index_infos() {
    with_state(
        "declare var a: { readonly [k: string]: number };\ndeclare var b: { [k: symbol]: number };\n",
        |state| {
            for (name, key) in [("a", TypeFlags::STRING), ("b", TypeFlags::ES_SYMBOL)] {
                let ty = annotation_type(state, name);
                let members_id = state
                    .resolve_structured_type_members(ty)
                    .expect("index members resolve");
                let members = state.members_of(members_id).clone();
                assert_eq!(members.index_infos().len(), 1);
                assert!(state
                    .tables
                    .flags_of(members.index_infos()[0].key_type)
                    .intersects(key));
                assert_eq!(members.index_infos()[0].is_readonly, name == "a");
            }
        },
    );
}

#[test]
fn template_annotations_fold_literal_spans() {
    with_state(
        "declare var a: `a${string}`;\ndeclare var b: `a${\"b\"}c`;\n",
        |state| {
            let a = annotation_type(state, "a");
            assert!(state
                .tables
                .flags_of(a)
                .intersects(TypeFlags::TEMPLATE_LITERAL));
            let b = annotation_type(state, "b");
            assert_eq!(b, state.tables.get_string_literal_type("abc"));
        },
    );
}

#[test]
fn intersection_normalization_matches_tsc() {
    with_state(
        concat!(
            "declare var a: string & number;\n",
            "declare var b: 1 & 2;\n",
            "declare var c: \"a\" & string;\n",
            "declare var d: string & {};\n",
            "declare var e: unknown & string;\n",
            "declare var f: (\"a\" | \"b\") & string;\n",
            "declare var g: (string | undefined) & (number | undefined);\n",
            "declare var h: boolean & true;\n",
            "declare var i: null & number;\n",
        ),
        |state| {
            let never = state.tables.intrinsics.never;
            // DisjointDomains: string & number = never (step 2).
            assert_eq!(annotation_type(state, "a"), never);
            // Unit ∧ Unit quirk: 1 & 2 = never.
            assert_eq!(annotation_type(state, "b"), never);
            // Supertype reduction: "a" & string = "a".
            let c = annotation_type(state, "c");
            assert_eq!(c, state.tables.get_string_literal_type("a"));
            // string & {} keeps both members (noSupertypeReduction).
            let d = annotation_type(state, "d");
            let TypeData::Intersection { types } = &state.tables.type_of(d).data else {
                panic!("string & {{}} stays an intersection");
            };
            assert_eq!(state.tables.list(*types).len(), 2);
            // unknown vanishes from intersections.
            assert_eq!(annotation_type(state, "e"), state.tables.intrinsics.string);
            // Union distribution: ("a"|"b") & string = "a" | "b".
            let f = annotation_type(state, "f");
            let a_lit = state.tables.get_string_literal_type("a");
            let b_lit = state.tables.get_string_literal_type("b");
            let expected = state.tables.get_union_type(
                None,
                &[a_lit, b_lit],
                tsc_types::UnionReduction::Literal,
            );
            assert_eq!(f, expected);
            // The undefined pull-out: (string|undefined) & (number|undefined)
            // = (string & number) | undefined = undefined.
            assert_eq!(
                annotation_type(state, "g"),
                state.tables.intrinsics.undefined
            );
            // Cross product over the boolean primitive union.
            assert_eq!(
                annotation_type(state, "h"),
                state.tables.intrinsics.true_regular
            );
            // strictNullChecks default-on: null & number is never
            // via the nullable∧NumberLike disjoint check.
            assert_eq!(annotation_type(state, "i"), never);
        },
    );
}

#[test]
fn intersections_are_insertion_order_sensitive_and_never_structural() {
    with_state(
        concat!(
            "declare var a: { x: number } & { y: string };\n",
            "declare var b: { y: string } & { x: number };\n",
            "declare var c: { x: number } & { x: number };\n",
        ),
        |state| {
            // Member order is identity: A & B differs from B & A.
            assert_ne!(annotation_type(state, "a"), annotation_type(state, "b"));
            // Structurally identical anonymous literals never dedup:
            // both members survive (the typeMembershipMap is
            // identity-keyed — the steps-doc 4.3 pin).
            let c = annotation_type(state, "c");
            let TypeData::Intersection { types } = &state.tables.type_of(c).data else {
                panic!("distinct {{x}} literals stay an intersection");
            };
            assert_eq!(state.tables.list(*types).len(), 2);
            assert_ne!(state.tables.list(*types)[0], state.tables.list(*types)[1]);
        },
    );
}

#[test]
fn resolved_conditional_and_unresolved_name_shapes_are_sound() {
    with_state(
        concat!(
            "declare var b: number extends string ? 1 : 2;\n",
            "declare var c: Missing;\n",
            "declare var d: Missing.Scope<string>;\n",
            "declare var e: Missing.Scope<string>;\n",
        ),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "b").expect("annotation");
            let conditional = state
                .get_type_from_type_node(annotation)
                .expect("resolved conditional");
            assert_eq!(
                state
                    .type_to_string(conditional)
                    .expect("resolved conditional display"),
                "2"
            );
            // Unresolved names are in-slice: resolveEntityName
            // reports 2304 and the reference types as
            // alias-bearing error intrinsics.
            let annotation =
                find_probe_annotation(state.binder.source(0), "c").expect("annotation");
            let ty = state
                .get_type_from_type_node(annotation)
                .expect("unresolved names type as an alias-bearing error");
            assert_ne!(ty, state.tables.intrinsics.error);
            assert!(state.tables.is_error_type(ty));
            let alias = state
                .tables
                .alias_symbol(ty)
                .expect("unresolved error retains its alias symbol");
            assert!(state
                .get_check_flags(alias)
                .intersects(CheckFlags::UNRESOLVED));
            assert_eq!(state.symbol_display_name(alias), "Missing");
            assert_eq!(
                state.type_to_string(ty).expect("unresolved alias display"),
                "Missing"
            );
            assert_eq!(
                state
                    .get_type_from_type_node(annotation)
                    .expect("negative type reference resolution is cached"),
                ty
            );
            assert!(matches!(
                state.links.node(annotation).resolved_symbol.get(),
                LinkSlot::Resolved(symbol) if symbol == alias
            ));

            let d = annotation_type(state, "d");
            let e = annotation_type(state, "e");
            assert_eq!(d, e, "same unresolved path and arguments intern");
            assert!(state.tables.is_error_type(d));
            let qualified_alias = state
                .tables
                .alias_symbol(d)
                .expect("qualified unresolved error retains its alias symbol");
            assert_eq!(
                state.get_fully_qualified_name(qualified_alias),
                "Missing.Scope"
            );
            assert_eq!(
                state
                    .type_to_string(d)
                    .expect("qualified unresolved alias display"),
                "Missing.Scope<string>"
            );
        },
    );
}

#[test]
fn type_node_resolution_is_stable_across_repeated_queries() {
    // tsc caches `links.resolvedType` in any check mode and never clears
    // it: a resolution a candidate completed stays published after the
    // candidate is discarded, and the next use is a cache hit on the same
    // type (unwinding it re-resolved and re-instantiated on every later use).
    with_state("declare var a: 1 | 2;\n", |state| {
        let annotation = find_probe_annotation(state.binder.source(0), "a")
            .expect("declared var with annotation");
        let resolved = annotation_type(state, "a");
        assert!(matches!(
            state.links.node(annotation).resolved_type.get(),
            LinkSlot::Resolved(cached) if cached == resolved
        ));
        assert_eq!(annotation_type(state, "a"), resolved);
    });
}

#[test]
fn deferred_reference_realization_retires_the_record_and_keeps_warmed_and_new_members() {
    with_state(
        "interface Base<T> { inherited: T; }\n\
         interface Box<T> extends Base<T> { plain: number; value: T; }\n\
         declare const box: Box<string>;",
        |state| {
            let ty = annotation_type(state, "box");
            let target = state.tables.reference_target(ty);
            let declared = state.resolve_declared_members(target).unwrap();
            let plain = state
                .members_of(declared)
                .members
                .get(&state.binder, "plain")
                .unwrap();
            // This identity decision must be frozen when headers are prepared.
            assert_eq!(
                state.get_type_of_symbol(plain).unwrap(),
                state.tables.intrinsics.number
            );
            state.structured_member_headers(ty).unwrap();
            assert!(state.lazy_members(ty).is_some());
            assert_eq!(
                state.get_property_of_object_type(ty, "plain").unwrap(),
                Some(plain)
            );
            let value = state
                .get_property_of_object_type(ty, "value")
                .unwrap()
                .unwrap();
            assert_eq!(
                state.get_type_of_symbol(value).unwrap(),
                state.tables.intrinsics.string
            );
            let all = state.get_properties_of_type_full(ty).unwrap();
            assert_eq!(all.len(), 3);
            assert!(all.contains(&plain));
            assert!(all.contains(&value));
            assert!(state.links.ty(ty).resolved_members.lazy() == crate::links::LazyMembers::None);
            assert!(state.members_resolved(ty));
            assert_eq!(
                state.get_property_of_object_type(ty, "value").unwrap(),
                Some(value)
            );
            assert_eq!(
                state.get_property_of_object_type(ty, "plain").unwrap(),
                Some(plain)
            );
            assert_eq!(state.get_properties_of_type_full(ty).unwrap(), all);
            assert!(state.links.ty(ty).resolved_members.lazy() == crate::links::LazyMembers::None);
        },
    );
}

#[test]
fn deferred_reference_headers_and_members_keep_their_ids_through_enumeration() {
    with_state(
        "interface Base<T> { [index: number]: T; inherited: T }
         interface Box<T> extends Base<T> {
             (arg: T): T; new(): Box<T>; plain: number;
             get(arg: T): T; get(): number;
         }
         declare let box: Box<string>;",
        |state| {
            let ty = annotation_type(state, "box");
            let source = state
                .resolve_declared_members(state.tables.reference_target(ty))
                .unwrap();
            let plain = state
                .members_of(source)
                .members
                .get(&state.binder, "plain")
                .unwrap();
            let header = state.structured_member_headers(ty).unwrap();
            let calls = state.members_of(header).call_signature_list();
            let constructs = state.members_of(header).construct_signature_list();
            let indices = state.members_of(header).index_info_list();
            assert_eq!((calls.len(), constructs.len(), indices.len()), (1, 1, 1));
            assert_eq!(indices[0].value_type, state.tables.intrinsics.string);
            state.get_type_of_symbol(plain).unwrap();
            let own = state
                .get_property_of_object_type(ty, "get")
                .unwrap()
                .unwrap();
            let inherited = state
                .get_property_of_object_type(ty, "inherited")
                .unwrap()
                .unwrap();
            let method = state.get_type_of_symbol(own).unwrap();
            let overloads = state
                .get_signatures_of_type(method, crate::state::SignatureKind::Call)
                .unwrap();
            assert_eq!(overloads.len(), 2);
            let counts = |state: &CheckerState<'_>| {
                let c = crate::CheckerCounts::of(state, true);
                (c.types, c.instantiations, c.symbols)
            };
            let before = counts(state);
            let members = state.get_properties_of_type_full(ty).unwrap();
            let after = counts(state);
            assert!(members.contains(&own) && members.contains(&inherited));
            assert!(!members.contains(&plain));
            assert_eq!(state.get_properties_of_type_full(ty).unwrap(), members);
            assert_eq!(state.structured_member_headers(ty).unwrap(), header);
            assert_eq!(state.members_of(header).call_signatures(), &*calls);
            assert_eq!(
                state.members_of(header).construct_signatures(),
                &*constructs
            );
            assert_eq!(
                state
                    .get_signatures_of_type(method, crate::state::SignatureKind::Call)
                    .unwrap(),
                overloads
            );
            assert_eq!(counts(state), after);
            assert_eq!((before, after), ((74, 4, 24), (74, 4, 25)));
        },
    );
}

#[test]
fn identity_members_freeze_aliases_and_select_inherited_properties_in_order() {
    with_state(
        "interface Base { base: boolean }
         interface Solo { (): number; own: string }
         interface Derived extends Base { (): number; own: string }",
        |state| {
            state.stable_type_ordering = false;
            for (name, inherits) in [("Solo", false), ("Derived", true)] {
                let owner = state.clone_symbol(*state.globals.get(name).unwrap());
                let own = state
                    .binder
                    .symbol(owner)
                    .members()
                    .get("own")
                    .copied()
                    .unwrap();
                let alias_name = tsc_types::known_name!("alias");
                let alias = state
                    .binder
                    .create_symbol(tsc_types::SymbolFlags::ALIAS, alias_name);
                state
                    .links
                    .set_fresh_symbol_alias_target(alias, LinkSlot::Resolved(own));
                std::sync::Arc::make_mut(state.binder.symbol_mut(owner).members_mut())
                    .insert(alias_name, alias);
                let ty = state.get_declared_type_of_symbol(owner).unwrap();
                let id = state.resolve_structured_type_members(ty).unwrap();
                let record = state.members_of(id);
                assert_eq!(record.members.get(&state.binder, alias_name), Some(alias));
                let mut expected = vec![own, alias];
                if inherits {
                    let base = *state.globals.get("Base").unwrap();
                    expected.push(*state.binder.symbol(base).members().get("base").unwrap());
                }
                assert_eq!(&*record.properties, expected);
                assert_eq!(record.call_signatures().len(), 1);
                assert_eq!(
                    record
                        .members
                        .get(&state.binder, tsc_types::InternalSymbolName::CALL)
                        .is_some(),
                    !inherits
                );
                assert_eq!(
                    record.members.get(&state.binder, "base").is_some(),
                    inherits
                );
            }
        },
    );
}

#[test]
fn eager_reference_members_pad_this_across_nested_resolution() {
    with_state(
        "interface Base<T> { self(): this; item: T }
         interface Derived<T> extends Base<T> { own: T }
         declare let d: Derived<string>;",
        |state| {
            let ty = annotation_type(state, "d");
            let id = state.resolve_structured_type_members(ty).unwrap();
            let record = state.members_of(id);
            let item = record.members.get(&state.binder, "item").unwrap();
            let own = record.members.get(&state.binder, "own").unwrap();
            let method = record.members.get(&state.binder, "self").unwrap();
            assert_eq!(
                state.get_type_of_symbol(item).unwrap(),
                state.tables.intrinsics.string
            );
            assert_eq!(
                state.get_type_of_symbol(own).unwrap(),
                state.tables.intrinsics.string
            );
            let method_ty = state.get_type_of_symbol(method).unwrap();
            let sigs = state
                .get_signatures_of_type(method_ty, crate::state::SignatureKind::Call)
                .unwrap();
            assert_eq!(state.get_return_type_of_signature(sigs[0]).unwrap(), ty);
            let counts = crate::CheckerCounts::of(state, true);
            assert_eq!(state.resolve_structured_type_members(ty), Ok(id));
            assert_eq!(
                state.get_property_of_object_type(ty, "self"),
                Ok(Some(method))
            );
            assert_eq!(
                state.get_signatures_of_type(method_ty, crate::state::SignatureKind::Call),
                Ok(sigs)
            );
            let repeated = crate::CheckerCounts::of(state, true);
            assert_eq!(
                (counts.types, counts.instantiations, counts.symbols),
                (repeated.types, repeated.instantiations, repeated.symbols)
            );
            assert_eq!(
                (counts.types, counts.instantiations, counts.symbols),
                (74, 6, 17)
            );
        },
    );
}

#[test]
fn recursive_mapped_member_lookup_observes_the_published_shell() {
    with_state(
        "type MapKeys<T> = { [K in keyof T]: MapKeys<T[K]> };
         interface Recursive { next: Recursive }
         declare let value: MapKeys<Recursive>;",
        |state| {
            let ty = annotation_type(state, "value");
            assert!(state
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::MAPPED));
            // Reenter after shell publication, while its key domain is being resolved.
            let shell = state.alloc_members(Default::default());
            state
                .links
                .set_type_members(state.speculation_depth, ty, LinkSlot::Resolved(shell));
            state.members_resolving += 1;
            assert_eq!(state.resolve_structured_type_members(ty), Ok(shell));
            assert!(state.get_properties_of_type_full(ty).unwrap().is_empty());
            state.members_resolving -= 1;
            state.links.retract_type_members(ty);
            let complete = state.resolve_structured_type_members(ty).unwrap();
            assert_ne!(shell, complete);
            let next = state
                .members_of(complete)
                .members
                .get(&state.binder, "next")
                .unwrap();
            assert_eq!(state.get_type_of_symbol(next), Ok(ty));
        },
    );
}

#[test]
fn declaration_routing_keeps_cross_file_overload_order() {
    crate::state::test_support::with_program_state(
        &[("a.ts", "function f(x: number): number;"),
          ("b.ts", "function f(x: string): string; function f(x: any) { return x; } let typed: string;")],
        &CompilerOptions::default(),
        |state| {
            let f = state.resolve_file_scope_name("f", tsc_types::SymbolFlags::VALUE).unwrap();
            let signatures = state.get_signatures_of_symbol(Some(f)).unwrap();
            let owners: Vec<_> = signatures.into_iter().map(|sig|
                state.binder.file_index_of_node(state.signature_of(sig).declaration.unwrap())
            ).collect();
            assert_eq!(owners, [0, 1]);
            let typed = state.resolve_file_scope_name("typed", tsc_types::SymbolFlags::VALUE).unwrap();
            assert_eq!(state.get_type_of_symbol(typed), Ok(state.tables.intrinsics.string));
        }
    );
}

#[test]
fn a_types_one_signature_stays_in_its_record_through_writes_and_the_weak_test() {
    with_state(
        "declare let callable: { (): void };
         declare let optional_callable: { (): void; opt?: number };
         declare let newable: { new (): object };
         declare let overloaded: { (): void; (x: number): void };",
        |state| {
            let callable = annotation_type(state, "callable");
            let id = state.resolve_structured_type_members(callable).unwrap();
            assert_eq!(state.members_of(id).call_signatures().len(), 1);
            assert_eq!(state.members_of(id).extras_heap_bytes(), 0);
            let signature = state.members_of(id).call_signatures()[0];
            // Opening the record for writing forgets a verdict, never the
            // signature its word holds.
            let _ = state.members_mut(id);
            assert_eq!(state.members_of(id).call_signatures(), &[signature]);
            // A type with a signature is not weak, and its word keeps the
            // signature: there is no room for a verdict, and none is written.
            assert!(!state.is_weak_type(callable).unwrap());
            assert!(!state.has_only_optional_properties(callable, id));
            assert_eq!(state.members_of(id).weak_properties(), None);
            assert_eq!(state.members_of(id).call_signatures(), &[signature]);

            let optional = annotation_type(state, "optional_callable");
            let id = state.resolve_structured_type_members(optional).unwrap();
            let signature = state.members_of(id).call_signatures()[0];
            assert!(!state.is_weak_type(optional).unwrap());
            // Its one property is optional; the answer is computed each time
            // and the signature stays.
            assert!(state.has_only_optional_properties(optional, id));
            assert!(state.has_only_optional_properties(optional, id));
            assert_eq!(state.members_of(id).weak_properties(), None);
            assert_eq!(state.members_of(id).call_signatures(), &[signature]);
            assert_eq!(state.members_of(id).extras_heap_bytes(), 0);

            let newable = annotation_type(state, "newable");
            let id = state.resolve_structured_type_members(newable).unwrap();
            assert!(state.members_of(id).call_signatures().is_empty());
            assert_eq!(state.members_of(id).construct_signatures().len(), 1);
            assert_eq!(state.members_of(id).extras_heap_bytes(), 0);

            let overloaded = annotation_type(state, "overloaded");
            let id = state.resolve_structured_type_members(overloaded).unwrap();
            assert_eq!(state.members_of(id).call_signatures().len(), 2);
            assert!(state.members_of(id).extras_heap_bytes() > 0);
        },
    );
}

#[test]
fn weak_properties_follow_the_full_record_through_resolution_and_speculation() {
    use tsc_types::SymbolFlags;

    with_state(
        "interface Base { required: number }
         interface Derived extends Base { opt?: number }
         declare let full: Derived;
         declare let shell: { opt?: number };
         interface Callable<T> { (): T; opt?: T }
         declare let deferred: Callable<string>;",
        |state| {
            let shell = annotation_type(state, "shell");
            let full = annotation_type(state, "full");
            let id = state.resolve_structured_type_members(shell).unwrap();
            let complete = state.resolve_structured_type_members(full).unwrap();
            let shared = state.create_resolved_empty_anonymous_type(None);
            state
                .links
                .set_type_members(0, shared, LinkSlot::Resolved(id));

            let counts = |state: &CheckerState| {
                let c = crate::CheckerCounts::of(state, true);
                (c.types, c.instantiations, c.symbols)
            };
            let before = counts(state);
            state.members_resolving += 1;
            assert!(state.is_weak_type(shell).unwrap());
            state.members_resolving -= 1;
            assert_eq!(state.members_of(id).weak_properties(), None);
            let checkpoint = state.begin_speculation();
            assert!(state.is_weak_type(shell).unwrap());
            state.rollback_speculation(checkpoint);
            assert_eq!(state.members_of(id).weak_properties(), None);
            assert!(state.is_weak_type(shell).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), Some(true));
            assert!(state.is_weak_type(shared).unwrap());
            assert_eq!(state.members_of(id).clone().weak_properties(), None);

            let inherited = state.members_of(complete).clone();
            state.publish_members(id, inherited);
            assert_eq!(state.members_of(id).weak_properties(), None);
            assert!(!state.is_weak_type(shared).unwrap());
            assert!(!state.is_weak_type(shell).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), Some(false));
            assert_eq!(state.members_of(id).clone().weak_properties(), None);
            assert_eq!(counts(state), before);

            let transient = state
                .binder
                .create_symbol(SymbolFlags::OPTIONAL, tsc_types::known_name!("name"));
            state.members_mut(id).set_properties(&[transient]);
            assert!(state.is_weak_type(shared).unwrap());
            state.binder.symbol_mut(transient).flags = SymbolFlags::PROPERTY;
            assert!(!state.is_weak_type(shell).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), None);
            let required = state
                .members_of(complete)
                .members
                .get(&state.binder, "required")
                .unwrap();
            state.binder.symbol_mut(transient).flags = SymbolFlags::OPTIONAL;
            state.members_mut(id).set_properties(&[transient, required]);
            assert!(!state.is_weak_type(shell).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), Some(false));
            let restored = state.members_of(id).clone();
            let checkpoint = state.begin_speculation();
            state.members_mut(id).set_properties(&[transient]);
            assert!(state.is_weak_type(shared).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), None);
            state.rollback_speculation(checkpoint);
            state.publish_members(id, restored);
            assert_eq!(state.members_of(id).weak_properties(), None);
            assert!(!state.is_weak_type(shared).unwrap());
            state.members_mut(id).set_properties(&[]);
            assert!(!state.is_weak_type(shared).unwrap());
            assert_eq!(state.members_of(id).weak_properties(), Some(false));

            let ty = annotation_type(state, "deferred");
            let header = state.structured_member_headers(ty).unwrap();
            let before = counts(state);
            assert!(!state.is_weak_type(ty).unwrap());
            assert!(state
                .links
                .read_ty(ty, |l| l.resolved_members.resolved())
                .is_none());
            assert_eq!(state.structured_member_headers(ty).unwrap(), header);
            let copy = state.members_of(header).clone();
            assert_eq!(
                copy.call_signatures(),
                state.members_of(header).call_signatures()
            );
            assert_eq!(copy.weak_properties(), None);
            assert_eq!(counts(state), before);
        },
    );
}

#[test]
fn signature_return_branches_preserve_target_precedence_composite_order_and_cycles() {
    let text = "declare function first(): { first: string };
declare function second(): { second: number };
function annotated(): string { return 'ok'; }
function unannotated() { return 0; }";
    with_state(text, |state| {
        use crate::state::Signature;
        use tsc_syntax::SyntaxKind;

        let nodes: Vec<_> = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .filter(|&node| state.kind_of(node) == SyntaxKind::FunctionDeclaration)
            .collect();
        let signatures: Vec<_> = nodes
            .into_iter()
            .map(|node| state.get_signature_from_declaration(node).unwrap())
            .collect();
        let [first, second, annotated, unannotated] = signatures[..] else {
            panic!("four signatures");
        };

        let mut target = Signature::new();
        target.target = Some(first);
        target.set_composite_signatures(Some(Box::new([second])));
        let target = state.alloc_signature(target);
        let first_return = state.get_return_type_of_signature(target).unwrap();
        assert_eq!(
            state.signature_of(first).resolved_return_type,
            Some(first_return)
        );
        assert!(state.signature_of(second).resolved_return_type.is_none());

        let mut composite = Signature::new();
        composite.set_composite_signatures(Some(Box::new([first, second])));
        composite.set_composite_kind(Some(TypeFlags::UNION));
        let composite = state.alloc_signature(composite);
        let combined = state.get_return_type_of_signature(composite).unwrap();
        let second_return = state.signature_of(second).resolved_return_type.unwrap();
        assert!(first_return.index() < second_return.index());
        assert!(second_return.index() < combined.index());

        for signature in [annotated, unannotated] {
            state.signature_mut(signature).target = Some(signature);
            assert_eq!(
                state.get_return_type_of_signature(signature).unwrap(),
                state.tables.intrinsics.any
            );
        }
        let rows: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .filter_map(|diag| diag.start.map(|start| (diag.code(), start as usize)))
            .collect();
        assert_eq!(
            rows,
            [
                (2577, text.find("): string").unwrap() + 3),
                (7023, text.find("unannotated").unwrap()),
            ]
        );
        assert!(state.resolution_targets.is_empty());
    });
}
