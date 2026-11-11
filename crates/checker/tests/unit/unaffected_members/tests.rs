use super::*;
use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use tsc_types::{CompilerOptions, TypeData};

fn annotation(state: &mut CheckerState<'_>, name: &str) -> TypeId {
    let node = find_probe_annotation(state.binder.source(0), name).unwrap();
    state.get_type_from_type_node(node).unwrap()
}

fn input(state: &mut CheckerState<'_>, name: &str) -> (TypeId, MembersId, MapperId, Vec<SymbolId>) {
    let reference = annotation(state, name);
    let target = state.tables.reference_target(reference);
    let table = state.resolve_declared_members(target).unwrap();
    let parameter = match state.tables.type_of(target).data {
        TypeData::GenericType {
            type_parameters, ..
        } => state.tables.list(type_parameters)[0],
        _ => panic!("generic target"),
    };
    let mapper = state.create_type_mapper(&[parameter], Some(&[state.tables.intrinsics.string]));
    (
        target,
        table,
        mapper,
        state.members_of(table).properties.to_vec(),
    )
}

fn symbol(state: &CheckerState<'_>, table: MembersId, name: &str) -> SymbolId {
    state
        .members_of(table)
        .members
        .get(&state.binder, name)
        .unwrap()
}

#[test]
fn pending_types_are_retried_without_changing_an_earlier_references_identity() {
    with_program_state(&[("a.ts", "interface Box<T> { plain: number; value: T } declare const a: Box<string>; declare const b: Box<number>;")], &CompilerOptions::default(), |state| {
        let (target, table, _, _) = input(state, "a");
        let plain = symbol(state, table, "plain");
        assert!(state.links.symbol(plain).type_of_symbol.resolved().is_none());
        let a = annotation(state, "a");
        state.structured_member_headers(a).unwrap();
        assert!(state.unaffected_members.entries[&target].pending.contains(&plain));
        state.get_type_of_symbol(plain).unwrap();
        let b = annotation(state, "b");
        state.structured_member_headers(b).unwrap();
        assert!(state.unaffected_members.entries[&target].settled.contains(&plain));
        assert_ne!(state.get_property_of_object_type(a, "plain").unwrap(), Some(plain));
        assert_eq!(state.get_property_of_object_type(b, "plain").unwrap(), Some(plain));
    });
}

#[test]
fn accessor_read_and_write_types_must_both_settle() {
    with_program_state(&[("a.ts", "interface Box<T> { get mixed(): number; set mixed(v: T); get fixed(): number; set fixed(v: number) } declare const a: Box<string>;")], &CompilerOptions::default(), |state| {
        let (target, table, mapper, properties) = input(state, "a");
        let mixed = symbol(state, table, "mixed");
        let fixed = symbol(state, table, "fixed");
        for sym in [mixed, fixed] {state.get_type_of_symbol(sym).unwrap();}
        assert!(state.links.symbol_cold().write_type.resolved(mixed).is_none());
        assert!(state.freeze_unaffected_members(target, table, mapper, &properties).is_empty());
        assert_eq!(state.unaffected_members.entries[&target].pending.len(), 2);
        state.get_write_type_of_accessors(mixed).unwrap();
        state.get_write_type_of_accessors(fixed).unwrap();
        assert_eq!(&*state.freeze_unaffected_members(target, table, mapper, &properties), &[fixed]);
        assert!(state.unaffected_members.entries[&target].pending.is_empty());
        let before = (state.tables.len(), state.binder.transient_symbol_count(), state.signatures.len(), state.instantiation_count, state.total_instantiation_count, state.diagnostics.attempts);
        assert_eq!(&*state.freeze_unaffected_members(target, table, mapper, &properties), &[fixed]);
        assert_eq!(before, (state.tables.len(), state.binder.transient_symbol_count(), state.signatures.len(), state.instantiation_count, state.total_instantiation_count, state.diagnostics.attempts));
    });
}

#[test]
fn this_only_mapper_is_part_of_the_stamp_and_unresolved_syntax_true_stays_pending() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { method(): number } declare const a: Box<string>;",
        )],
        &CompilerOptions::default(),
        |state| {
            let (target, table, general, properties) = input(state, "a");
            let reference = annotation(state, "a");
            let this_type = match state.tables.type_of(target).data {
                TypeData::GenericType { this_type, .. } => this_type,
                _ => unreachable!(),
            };
            let this_mapper = state.create_type_mapper(&[this_type], Some(&[reference]));
            let method = properties[0];
            assert!(state.maps_this_only(this_mapper));
            assert_eq!(
                &*state.freeze_unaffected_members(target, table, this_mapper, &properties),
                &[method]
            );
            assert_eq!(
                &*state.unaffected_members.entries[&target].pending,
                &[method]
            );
            state.get_type_of_symbol(method).unwrap();
            assert_eq!(
                &*state.freeze_unaffected_members(target, table, this_mapper, &properties),
                &[method]
            );
            assert!(state.unaffected_members.entries[&target].pending.is_empty());
            assert!(state
                .freeze_unaffected_members(target, table, general, &properties)
                .is_empty());
            assert!(!state.unaffected_members.entries[&target].maps_this_only);
        },
    );
}

#[test]
fn merged_table_identity_length_and_reset_epoch_each_invalidate() {
    with_program_state(&[("a.ts", "interface Box<T> { one: number } interface Box<T> { two: T } declare const a: Box<string>;")], &CompilerOptions::default(), |state| {
        let (target, table, mapper, properties) = input(state, "a");
        assert_eq!(properties.len(), 2);
        for &sym in &properties {state.get_type_of_symbol(sym).unwrap();}
        let expected = state.fresh_unaffected_members(&properties, mapper);
        assert_eq!(state.freeze_unaffected_members(target, table, mapper, &properties), expected);
        let replacement = state.alloc_members(state.members_of(table).clone());
        assert_eq!(state.freeze_unaffected_members(target, replacement, mapper, &properties), expected);
        assert_eq!(state.unaffected_members.entries[&target].table, replacement);
        assert_eq!(state.freeze_unaffected_members(target, replacement, mapper, &properties[..1]), state.fresh_unaffected_members(&properties[..1], mapper));
        assert_eq!(state.unaffected_members.entries[&target].len, 1);
        state.links.members_reset_epoch += 1;
        assert_eq!(state.freeze_unaffected_members(target, table, mapper, &properties), expected);
        assert_eq!(state.unaffected_members.epoch, state.links.members_reset_epoch);
        assert_eq!(state.unaffected_members.entries.len(), 1);
    });
}

#[test]
fn byte_cap_falls_through_to_the_original_decision() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { plain: number; value: T } declare const a: Box<string>;",
        )],
        &CompilerOptions::default(),
        |state| {
            let (target, table, mapper, properties) = input(state, "a");
            for &sym in &properties {
                state.get_type_of_symbol(sym).unwrap();
            }
            let expected = state.fresh_unaffected_members(&properties, mapper);
            state.unaffected_members.payload_bytes = MAX_BYTES;
            assert_eq!(
                state.freeze_unaffected_members(target, table, mapper, &properties),
                expected
            );
            assert!(state.unaffected_members.entries.is_empty());
            state.unaffected_members.payload_bytes = 0;
            assert_eq!(
                state.freeze_unaffected_members(target, table, mapper, &properties),
                expected
            );
            assert!(state.unaffected_members.heap_bytes() <= MAX_BYTES);
        },
    );
}

#[test]
fn entry_cap_falls_through_without_evicting_another_targets_decisions() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { plain: number } declare const a: Box<string>;",
        )],
        &CompilerOptions::default(),
        |state| {
            let (target, table, mapper, properties) = input(state, "a");
            state.get_type_of_symbol(properties[0]).unwrap();
            let expected = state.fresh_unaffected_members(&properties, mapper);
            for index in 0..MAX_ENTRIES {
                state.unaffected_members.entries.insert(
                    TypeId::new(1_000_000 + index as u32),
                    Entry {
                        table,
                        len: 0,
                        maps_this_only: false,
                        settled: Box::new([]),
                        pending: Box::new([]),
                    },
                );
            }
            state.unaffected_members.map_buckets =
                (state.unaffected_members.entries.capacity() + 1).next_power_of_two();
            assert_eq!(
                state.freeze_unaffected_members(target, table, mapper, &properties),
                expected
            );
            assert_eq!(state.unaffected_members.entries.len(), MAX_ENTRIES);
            assert!(!state.unaffected_members.entries.contains_key(&target));
            assert!(state.unaffected_members.heap_bytes() <= MAX_BYTES);
        },
    );
}
