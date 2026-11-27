use super::*;

#[test]
fn symbol_flags_projection_preserves_defaults_and_every_transient_bit() {
    let mut links = LinksTables::default();
    let persistent = [0, 257, (1 << 26) - 1, 1 << 26, (1 << 31) - 1];
    let before = links.memory_usage();
    for raw in persistent {
        assert_eq!(links.read_symbol_check_flags(SymbolId::new(raw)).bits(), 0);
    }
    let missing_transient = SymbolId::new(u32::MAX - 1);
    assert_eq!(links.read_symbol_check_flags(missing_transient).bits(), 0);
    assert_eq!(links.memory_usage(), before);

    // A populated type cache still gives a persistent symbol no flags.
    let symbol = SymbolId::new(257);
    links.set_symbol_type(0, symbol, LinkSlot::Resolved(TypeId::new(9)));
    assert_eq!(links.read_symbol_check_flags(symbol).bits(), 0);
    assert_eq!(
        links.symbol(symbol).type_of_symbol.resolved(),
        Some(TypeId::new(9))
    );

    let symbols = [0, 17, 255].map(|index| SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index));
    for (symbol, bits) in symbols.into_iter().zip([0, i32::MIN, -1]) {
        links.set_symbol_check_flags(0, symbol, tsc_types::CheckFlags::from_bits(bits));
    }
    for (symbol, bits) in symbols.into_iter().zip([0, i32::MIN, -1]) {
        assert_eq!(links.read_symbol_check_flags(symbol).bits(), bits);
        assert_eq!(
            links.read_symbol_check_flags(symbol),
            links.symbol(symbol).check_flags
        );
    }
    assert_eq!(links.read_symbol_check_flags(missing_transient).bits(), 0);
}

#[test]
fn value_slots_keep_replacements_clearing_and_identity_bounds() {
    fn check<T: SlotId + std::fmt::Debug + Eq>() {
        assert_eq!(std::mem::size_of::<ValueSlot<T>>(), 4);
        let mut slot = ValueSlot::<T>::default();
        let mut original = LinkSlot::Vacant;
        for next in [
            LinkSlot::Vacant,
            LinkSlot::Resolved(T::from_slot_index(0)),
            LinkSlot::Resolved(T::from_slot_index(1)),
            LinkSlot::Resolved(T::from_slot_index(u32::MAX - 2)),
            LinkSlot::Vacant,
            LinkSlot::Vacant,
            LinkSlot::Resolved(T::from_slot_index(0)),
        ] {
            LinksTables::write_slot(&mut original, next.clone());
            LinksTables::write_value_slot(&mut slot, next);
            assert_eq!(slot.get(), original);
            assert_eq!(slot.resolved(), original.resolved());
            assert!(!slot.is_resolving());
            assert_eq!(format!("{slot:?}"), format!("{original:?}"));
        }
    }
    let initial = debug_resolving_open();
    check::<TypeId>();
    check::<SymbolId>();
    check::<MembersId>();
    assert_eq!(debug_resolving_open(), initial);
    assert_eq!(std::mem::size_of::<NodeLinks>(), 16);
    assert_eq!(std::mem::size_of::<TypeLinks>(), 12);
}

#[test]
fn a_members_slot_holds_resolved_members_or_a_lazy_table_in_four_bytes() {
    assert_eq!(std::mem::size_of::<MembersSlot>(), 4);
    let mut links = LinksTables::default();
    let (a, b, c) = (TypeId::new(3), TypeId::new(4), TypeId::new(900));
    let (first, second) = (MembersId::new(0), MembersId::new((1 << 31) - 2));
    // Vacant: no members, no lazy table; an unwritten type reads the same.
    for ty in [a, b, c] {
        assert_eq!(links.ty(ty).resolved_members.resolved(), None);
        assert_eq!(links.ty(ty).resolved_members.lazy(), LazyMembers::None);
    }
    // In preparation, then ready by index: still "not resolved" to every
    // reader of the members.
    links.begin_type_lazy_members(a);
    assert_eq!(links.ty(a).resolved_members.lazy(), LazyMembers::Preparing);
    assert_eq!(links.ty(a).resolved_members.resolved(), None);
    assert!(matches!(
        links.ty(a).resolved_members.get(),
        LinkSlot::Vacant
    ));
    for index in [0, 7, (1 << 31) - 3] {
        links.end_type_lazy_members(b);
        links.begin_type_lazy_members(b);
        links.set_type_lazy_members(b, index);
        assert_eq!(
            links.ty(b).resolved_members.lazy(),
            LazyMembers::Ready(index)
        );
        assert_eq!(links.ty(b).resolved_members.resolved(), None);
        assert!(matches!(
            links.ty(b).resolved_members.get(),
            LinkSlot::Vacant
        ));
    }
    // Resolved members take the slot from either lazy state, and giving a
    // lazy table up afterwards leaves them.
    links.set_type_members(0, a, LinkSlot::Resolved(first));
    links.set_type_members(0, b, LinkSlot::Resolved(second));
    for (ty, members) in [(a, first), (b, second)] {
        assert_eq!(links.ty(ty).resolved_members.resolved(), Some(members));
        assert_eq!(links.ty(ty).resolved_members.lazy(), LazyMembers::None);
        links.end_type_lazy_members(ty);
        assert_eq!(links.ty(ty).resolved_members.resolved(), Some(members));
    }
    // A table given up in preparation, or when ready, leaves the slot vacant.
    links.begin_type_lazy_members(c);
    links.end_type_lazy_members(c);
    assert_eq!(links.ty(c).resolved_members.lazy(), LazyMembers::None);
    links.begin_type_lazy_members(c);
    links.set_type_lazy_members(c, 1);
    links.end_type_lazy_members(c);
    assert_eq!(links.ty(c).resolved_members.lazy(), LazyMembers::None);
    assert_eq!(links.ty(c).resolved_members.resolved(), None);
    // The retraction of resolved members is as before.
    links.retract_type_members(a);
    assert_eq!(links.ty(a).resolved_members.resolved(), None);
    assert_eq!(links.ty(a).resolved_members.lazy(), LazyMembers::None);
}

#[test]
#[should_panic(expected = "value-only identity slot cannot store Resolving")]
fn value_slots_reject_a_separate_in_progress_state() {
    let _ = ValueSlot::<TypeId>::from(LinkSlot::Resolving);
}

#[test]
#[should_panic(expected = "does not fit a link slot")]
fn value_slots_keep_the_existing_identity_limit() {
    let _ = ValueSlot::from(LinkSlot::Resolved(TypeId::new(u32::MAX - 1)));
}

#[derive(Clone, Copy)]
struct Key(usize);

impl DenseKey for Key {
    fn dense_index(self) -> usize {
        self.0
    }
}

#[test]
fn located_records_observe_replacements_and_clearing_after_directory_growth() {
    let far_region = (DENSE_PAGES << PAGE_SHIFT) >> REGION_SHIFT;
    for first_region in [3, far_region] {
        let mut table = PagedTable::<Key, Option<u32>>::default();
        let first = Key((first_region << REGION_SHIFT) + 71);
        *table.slot(first) = Some(9);
        let position = table.get_with_position(first).unwrap().0;
        for region in [0, 3, 9, far_region + 1] {
            for offset in 0..600 {
                let key = Key((region << REGION_SHIFT) + offset);
                if key.0 != first.0 {
                    *table.slot(key) = Some(key.0 as u32);
                }
            }
        }
        assert_eq!(table.get_at_position(position), &Some(9));
        *table.slot(first) = None;
        assert_eq!(table.get_at_position(position), &None);
        *table.slot(first) = Some(42);
        assert_eq!(table.get_at_position(position), &Some(42));
        assert_eq!(table.get(first), Some(&Some(42)));
    }
}

#[test]
fn paged_table_fills_a_region_and_keeps_records_stable() {
    let region_ids = 1 << REGION_SHIFT;
    let mut table = PagedTable::<Key, usize>::with_id_bound(region_ids);
    let directories = directory_state(&table);
    // An odd multiplier permutes every id, so record slots differ from ids.
    let id = |ordinal| (ordinal * 109 + 71) & (region_ids - 1);
    let first = Key(id(0));
    assert_eq!(table.get(first), None);
    assert_eq!(table.memory_usage().0, 0);
    assert_eq!(allocated_chunks(&table), 0);
    *table.slot(first) = 1;
    let address = std::ptr::from_ref(table.get(first).unwrap());
    for ordinal in 1..region_ids {
        let key = Key(id(ordinal));
        assert_eq!(table.get(key), None);
        *table.slot(key) = ordinal + 1;
        assert_eq!(allocated_chunks(&table), ordinal / CHUNK_RECORDS + 1);
    }
    assert_eq!(table.len, region_ids);
    assert_eq!(table.region_lens[0], 32_768);
    assert_eq!(table.memory_usage().0, region_ids);
    assert_eq!(std::ptr::from_ref(table.get(first).unwrap()), address);
    for ordinal in 0..region_ids {
        let key = Key(id(ordinal));
        assert_eq!(table.get(key), Some(&(ordinal + 1)));
        assert_eq!(*table.slot(key), ordinal + 1);
    }
    assert_eq!(table.len, region_ids);
    assert_eq!(table.get(Key(region_ids)), None);
    assert_eq!(directory_state(&table), directories);
}

#[test]
fn paged_table_interleaves_regions_without_aliasing() {
    let mut table = PagedTable::<Key, usize>::with_id_bound((7 << REGION_SHIFT) + 1);
    let directories = directory_state(&table);
    let regions = [3, 0, 1];
    // Reverse id order and interleave regions across multiple record chunks.
    for ordinal in 0..600 {
        for region in regions {
            let id = (region << REGION_SHIFT) + ((1 << REGION_SHIFT) - 1 - ordinal);
            *table.slot(Key(id)) = id + 1;
        }
    }
    let first = Key((3 << REGION_SHIFT) + (1 << REGION_SHIFT) - 1);
    let address = std::ptr::from_ref(table.get(first).unwrap());
    *table.slot(Key(7 << REGION_SHIFT)) = 42;
    assert_eq!(std::ptr::from_ref(table.get(first).unwrap()), address);
    for ordinal in 0..600 {
        for region in regions {
            let id = (region << REGION_SHIFT) + ((1 << REGION_SHIFT) - 1 - ordinal);
            assert_eq!(table.get(Key(id)), Some(&(id + 1)));
            assert_eq!(*table.slot(Key(id)), id + 1);
        }
    }
    assert_eq!(table.get(Key(2 << REGION_SHIFT)), None);
    assert_eq!(table.get(Key(7 << REGION_SHIFT)), Some(&42));
    assert_eq!(table.len, 1801);
    assert_eq!(
        allocated_chunks(&table),
        3 * 600usize.div_ceil(CHUNK_RECORDS) + 1
    );
    assert_eq!(directory_state(&table), directories);
}

#[test]
fn symbol_type_lookup_distinguishes_dense_far_and_transient_ids() {
    let mut links = LinksTables::default();
    let far_start = (DENSE_PAGES * PAGE_IDS) as u32;
    let persistent = SymbolId::new(3);
    let far = SymbolId::new(far_start);
    let transient = SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | 5);
    let ty = TypeId::new(7);
    for id in [persistent, transient, SymbolId::new(far_start - 1)] {
        assert_eq!(links.lookup_symbol_type(id), SymbolTypeLookup::Common(None));
    }
    assert_eq!(links.lookup_symbol_type(far), SymbolTypeLookup::Far);
    assert_eq!(links.symbol(far).type_of_symbol.resolved(), None);
    for id in [persistent, transient, far] {
        links.set_fresh_symbol_type(id, LinkSlot::Resolved(ty));
        assert_eq!(links.symbol(id).type_of_symbol.resolved(), Some(ty));
    }
    for id in [persistent, transient] {
        assert_eq!(
            links.lookup_symbol_type(id),
            SymbolTypeLookup::Common(Some(ty))
        );
    }
    assert_eq!(links.lookup_symbol_type(far), SymbolTypeLookup::Far);
    assert_eq!(
        links.lookup_symbol_type(SymbolId::new(4)),
        SymbolTypeLookup::Common(None)
    );

    crate::state::test_support::with_program_state(
        &[("a.ts", "")],
        &tsc_types::CompilerOptions::default(),
        |state| {
            let expected = state.tables.intrinsics.string;
            state
                .links
                .set_fresh_symbol_type(far, LinkSlot::Resolved(expected));
            assert_eq!(state.get_type_of_symbol(far).unwrap(), expected);
        },
    );
}

#[test]
fn paged_table_far_pages_use_the_same_region_slots() {
    let far_id = DENSE_PAGES << PAGE_SHIFT;
    let next_region = far_id + (1 << REGION_SHIFT);
    let mut table = PagedTable::<Key, usize>::with_id_bound(next_region + 1);
    let directories = directory_state(&table);
    assert_eq!(table.get(Key(far_id)), None);
    assert!(table.far.is_empty());
    *table.slot(Key(0)) = 1;
    for offset in 0..300 {
        *table.slot(Key(far_id + offset)) = offset + 2;
    }
    *table.slot(Key(next_region)) = 999;
    for offset in 0..300 {
        assert_eq!(table.get(Key(far_id + offset)), Some(&(offset + 2)));
        assert_eq!(*table.slot(Key(far_id + offset)), offset + 2);
    }
    assert_eq!(table.get(Key(far_id + 300)), None);
    assert_eq!(table.get(Key(next_region)), Some(&999));
    assert_eq!(table.get(Key(0)), Some(&1));
    assert_eq!(table.index.len(), DENSE_PAGES);
    assert_eq!(table.far.len(), 3);
    assert_eq!(table.len, 302);
    assert_eq!(allocated_chunks(&table), 4);
    assert_eq!(directory_state(&table), directories);

    // The public default constructor still accepts keys without a bound.
    let mut unbounded = PagedTable::<Key, usize>::default();
    *unbounded.slot(Key(far_id)) = 7;
    *unbounded.slot(Key(0)) = 8;
    *unbounded.slot(Key(next_region)) = 9;
    assert_eq!(unbounded.get(Key(far_id)), Some(&7));
    assert_eq!(unbounded.get(Key(0)), Some(&8));
    assert_eq!(unbounded.get(Key(next_region)), Some(&9));
}

fn allocated_chunks(table: &PagedTable<Key, usize>) -> usize {
    table.chunks.iter().filter(|chunk| chunk.is_some()).count()
}

fn directory_state(table: &PagedTable<Key, usize>) -> [(usize, usize, usize); 3] {
    [
        (
            table.index.as_ptr() as usize,
            table.index.len(),
            table.index.capacity(),
        ),
        (
            table.chunks.as_ptr() as usize,
            table.chunks.len(),
            table.chunks.capacity(),
        ),
        (
            table.region_lens.as_ptr() as usize,
            table.region_lens.len(),
            table.region_lens.capacity(),
        ),
    ]
}

#[test]
fn finished_paged_links_release_their_pages_and_keep_other_link_owners() {
    let mut links = LinksTables::with_id_bounds((1 << REGION_SHIFT) + 1, PAGE_IDS + 1);
    let ty = TypeId::new(7);
    for index in 0..=CHUNK_RECORDS {
        links.set_node_resolved_type(0, NodeId::new(index as u32), LinkSlot::Resolved(ty));
    }
    links.set_node_resolved_type(0, NodeId::new(1 << REGION_SHIFT), LinkSlot::Resolved(ty));
    links.set_symbol_type(0, SymbolId::new(1), LinkSlot::Resolved(ty));
    links.set_symbol_type(0, SymbolId::new(PAGE_IDS as u32), LinkSlot::Resolved(ty));
    links.set_node_context_free_type(NodeId::new(3), LinkSlot::Resolved(ty));
    links.ty.slot(ty).type_parameter_constraint = LinkSlot::Resolved(ty).into();
    links.transient_symbols.push(SymbolLinks::default());
    let cold = links.node_cold.memory_usage();
    let dense = links.ty.memory_usage();
    assert_eq!(
        links
            .node
            .chunks
            .iter()
            .filter(|chunk| chunk.is_some())
            .count(),
        3
    );
    assert_eq!(
        links
            .persistent_symbols
            .index
            .iter()
            .filter(|page| page.is_some())
            .count(),
        2
    );

    links.release_paged_record_buffers();

    assert_eq!(links.node.memory_usage(), (0, 0));
    assert_eq!(links.persistent_symbols.memory_usage(), (0, 0));
    assert_eq!(links.node_cold.memory_usage(), cold);
    assert_eq!(links.ty.memory_usage(), dense);
    assert_eq!(links.transient_symbols.len(), 1);
}

#[test]
fn outer_parameters_keep_empty_lists_and_overwrites_across_collisions() {
    let mut links = LinksTables::default();
    let a = NodeId::new(5);
    let b = NodeId::new(a.index() + OuterTypeParameters::SLOTS as u32);
    let empty = tsc_types::TypeListId::new(0);
    let list = tsc_types::TypeListId::new(17);
    assert_eq!(links.node_outer_type_parameters(a), None);
    links.set_node_outer_type_parameters(a, empty);
    assert_eq!(links.node_outer_type_parameters(b), None);
    links.set_node_outer_type_parameters(b, list);
    assert_eq!(links.node_outer_type_parameters(a), Some(empty));
    assert_eq!(
        links.node_cold.outer_type_parameters.slots[5],
        Some((a, empty))
    );
    assert_eq!(links.node_outer_type_parameters(b), Some(list));
    links.set_node_outer_type_parameters(b, empty);
    assert_eq!(links.node_outer_type_parameters(b), Some(empty));
    assert_eq!(links.node_outer_type_parameters(a), Some(empty));
    assert_eq!(links.node_outer_type_parameters(b), Some(empty));
}

#[test]
fn mapped_slots_share_records_but_keep_independent_transitions() {
    let mut links = LinksTables::default();
    let first = TypeId::new(14);
    let second = TypeId::new(15);
    let value = TypeId::new(u32::MAX - 1);
    let initial = debug_resolving_open();
    assert_eq!(links.type_cold.mapped(first).constraint, LinkSlot::Vacant);
    assert_eq!(links.type_cold.mapped_types.len, 0);
    links.set_fresh_mapped_type_parameter(first, value);
    LinksTables::write_slot(
        &mut links.type_cold.mapped_types.slot(first).constraint,
        LinkSlot::Resolving,
    );
    assert_eq!(
        links.type_cold.mapped(first).constraint,
        LinkSlot::Resolving
    );
    links.set_mapped_constraint_type(first, value);
    links.set_mapped_template_type(first, value);
    links.set_mapped_modifiers_type(first, value);
    links.set_mapped_template_type(first, second);
    assert_eq!(links.type_cold.mapped_types.len, 1);
    assert_eq!(
        links.type_cold.mapped(first).parameter.resolved(),
        Some(value)
    );
    assert_eq!(
        links.type_cold.mapped(first).constraint.resolved(),
        Some(value)
    );
    assert_eq!(
        links.type_cold.mapped(first).modifiers.resolved(),
        Some(value)
    );
    assert_eq!(
        links.type_cold.mapped(first).template.resolved(),
        Some(second)
    );
    assert_eq!(links.type_cold.mapped(second).template, LinkSlot::Vacant);
    assert_eq!(debug_resolving_open(), initial);
}

#[test]
fn call_signature_journal_index_survives_reentry_and_rejects_reused_slots() {
    let run = |use_index: bool, commit: bool| {
        let mut links = LinksTables::default();
        let node = NodeId::new(3);
        let other = NodeId::new(9);
        let mut cursor = NodeLinksCursor::new(node);
        let outer = links.speculative_marks();
        let saved = cursor.set_call_signature(&mut links, 1, LinkSlot::Resolving, None);
        let nested = links.speculative_marks();
        let hint = |entry| if use_index { entry } else { None };
        // The outer entry refers to the right node at a different depth.
        let inner = cursor.set_call_signature(&mut links, 2, LinkSlot::Resolving, hint(saved));
        let result = LinkSlot::Resolved(SignatureId::new(11));
        cursor.set_call_signature(&mut links, 2, result.clone(), hint(inner));
        if commit {
            links.commit_speculative_writes(nested, 1);
        } else {
            links.restore_speculative_writes(nested);
        }
        // The popped inner index is now out of bounds.
        cursor.set_call_signature(&mut links, 1, result.clone(), hint(inner));
        assert_eq!(links.speculative_resolved_signature_mark(), 1);
        // Reuse the popped inner index at the same depth for another node.
        let mut other_cursor = NodeLinksCursor::new(other);
        other_cursor.set_call_signature(&mut links, 2, LinkSlot::Resolving, None);
        cursor.set_call_signature(&mut links, 2, result.clone(), hint(inner));
        let journal = links.speculative_resolved_signature_writes.clone();
        assert_eq!(journal.len(), 3);
        assert_eq!(journal[2], (2, node, result.clone()));
        links.restore_speculative_writes(nested);
        cursor.set_call_signature(&mut links, 1, result.clone(), hint(saved));
        links.restore_speculative_writes(outer);
        assert_eq!(links.speculative_resolved_signature_mark(), 0);
        assert_eq!(links.node(other).resolved_signature.get(), LinkSlot::Vacant);
        (journal, links.node(node).resolved_signature.get())
    };
    for commit in [false, true] {
        let ordinary = run(false, commit);
        assert_eq!(ordinary.1, LinkSlot::Resolved(SignatureId::new(11)));
        assert_eq!(run(true, commit), ordinary);
    }
}

#[test]
fn type_reference_probe_keeps_absent_records_unallocated_and_reads_live_slots() {
    let mut links = LinksTables::default();
    let before = links.memory_usage();
    let node = NodeId::new(3);
    let mut outer = NodeLinksCursor::probe_type_reference(&links, node).expect_err("no record");
    assert!(outer.location.is_none());
    assert_eq!(links.memory_usage(), before);

    let inner_symbol = SymbolId::new(5);
    let inner_type = TypeId::new(6);
    outer.set_symbol(&mut links, inner_symbol);
    let mut inner =
        NodeLinksCursor::probe_type_reference(&links, node).expect_err("record without type");
    assert_eq!(inner.location, outer.location);
    inner.finish_type_reference(&mut links, Some(inner_symbol), inner_type);
    assert_eq!(
        NodeLinksCursor::probe_type_reference(&links, node).ok(),
        Some(inner_type)
    );

    let outer_symbol = SymbolId::new(7);
    let outer_type = TypeId::new(8);
    outer.finish_type_reference(&mut links, Some(outer_symbol), outer_type);
    assert_eq!(
        NodeLinksCursor::probe_type_reference(&links, node).ok(),
        Some(outer_type)
    );
    assert_eq!(
        inner.read(&links, |record| record.resolved_symbol.resolved()),
        Some(outer_symbol)
    );
}

/// Values written as their symbols are created lie in creation order and are
/// found by rank across the words of bits; a symbol between two written ones
/// reads the default; a second write replaces in place and returns what was
/// there.
#[test]
fn creation_order_links_find_appended_values_by_rank() {
    let transient = |index: u32| SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index);
    let mut links = CreationOrderLinks::<SymbolId, Option<TypeId>>::default();
    assert_eq!(links.memory_usage(), (0, 0));
    assert_eq!(*links.get(transient(0)), None);
    let written = [0u32, 1, 63, 64, 65, 127, 128, 1_000, 1_001, 70_000];
    for (rank, index) in written.into_iter().enumerate() {
        assert_eq!(
            links.replace(transient(index), Some(TypeId::new(index + 1))),
            None
        );
        assert_eq!(links.values.len(), rank + 1);
    }
    for index in 0..70_100u32 {
        let expected = written.contains(&index).then(|| TypeId::new(index + 1));
        assert_eq!(*links.get(transient(index)), expected, "{index}");
    }
    assert!(links.late.is_empty());
    assert_eq!(links.memory_usage().0, written.len());

    // In place: the previous value comes back and nothing is appended.
    assert_eq!(
        links.replace(transient(64), Some(TypeId::new(7))),
        Some(TypeId::new(65))
    );
    assert_eq!(*links.get(transient(64)), Some(TypeId::new(7)));
    assert_eq!(*links.get(transient(65)), Some(TypeId::new(66)));
    assert_eq!(links.values.len(), written.len());

    // A field of types ranks every type: a fresh deferred reference's mapper.
    let mut tables = LinksTables::default();
    let mapper = crate::instantiate::MapperId::new;
    for index in [10u32, 11, 12, 90, 4_000] {
        tables.set_fresh_deferred_mapper(TypeId::new(index), Some(mapper(index + 1)));
    }
    // A reference without a mapper stores nothing.
    tables.set_fresh_deferred_mapper(TypeId::new(4_001), None);
    let cold = tables.type_cold();
    for index in 0..4_100u32 {
        let expected = [10u32, 11, 12, 90, 4_000]
            .contains(&index)
            .then(|| mapper(index + 1));
        assert_eq!(*cold.deferred_mapper.get(TypeId::new(index)), expected);
    }
    assert_eq!(cold.deferred_mapper.values.len(), 5);
    assert!(cold.deferred_mapper.late.is_empty());
}

/// A write below a symbol that already has a value, and a persistent
/// symbol's, are kept beside the values and read the same; storing the
/// default unsets either kind.
#[test]
fn creation_order_links_keep_out_of_order_and_persistent_writes() {
    let transient = |index: u32| SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index);
    let persistent = SymbolId::new(42);
    let mut links = CreationOrderLinks::<SymbolId, Option<TypeId>>::default();
    links.set(transient(100), Some(TypeId::new(1)));
    links.set(transient(200), Some(TypeId::new(2)));
    // Below the last written symbol: not an append.
    links.set(transient(150), Some(TypeId::new(3)));
    links.set(persistent, Some(TypeId::new(4)));
    assert_eq!(links.values.len(), 2);
    assert_eq!(links.late.len(), 2);
    assert_eq!(*links.get(transient(100)), Some(TypeId::new(1)));
    assert_eq!(*links.get(transient(150)), Some(TypeId::new(3)));
    assert_eq!(*links.get(transient(200)), Some(TypeId::new(2)));
    assert_eq!(*links.get(persistent), Some(TypeId::new(4)));
    assert_eq!(*links.get(SymbolId::new(43)), None);
    assert_eq!(*links.get(transient(151)), None);
    assert_eq!(
        links.replace(transient(150), Some(TypeId::new(5))),
        Some(TypeId::new(3))
    );
    assert_eq!(
        links.replace(persistent, Some(TypeId::new(6))),
        Some(TypeId::new(4))
    );
    assert_eq!(links.memory_usage().0, 4);

    // The default unsets: a value among the appended ones keeps its place,
    // one beside them goes.
    links.set(transient(100), None);
    links.set(transient(150), None);
    links.set(persistent, None);
    assert_eq!(*links.get(transient(100)), None);
    assert_eq!(*links.get(transient(150)), None);
    assert_eq!(*links.get(persistent), None);
    assert_eq!(*links.get(transient(200)), Some(TypeId::new(2)));
    assert!(links.late.is_empty());

    // Unset, then written again: in place, and it reads as a first write.
    assert_eq!(links.replace(transient(100), Some(TypeId::new(8))), None);
    assert_eq!(*links.get(transient(100)), Some(TypeId::new(8)));
    assert_eq!(links.values.len(), 2);

    // Storing the default for a symbol that has nothing writes nothing.
    links.set(transient(5_000), None);
    assert_eq!(links.values.len(), 2);
    assert_eq!(links.end, 201);
}

/// The members a mapped type synthesizes keep their links in creation order:
/// each member's mapped type, key type and synthetic origin are read back by
/// its symbol, a duplicate remap replaces the key type in place, and a
/// reverse-mapped member shares the mapped-type field.
#[test]
fn mapped_member_links_are_read_back_by_symbol_in_creation_order() {
    let transient = |index: u32| SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index);
    let mut links = LinksTables::default();
    let mapped = TypeId::new(500);
    let members = [3u32, 4, 5, 70, 200];
    for index in members {
        links.set_symbol_mapped_links(
            0,
            transient(index),
            mapped,
            TypeId::new(1_000 + index),
            TypeId::new(2_000 + index),
        );
        if index != 70 {
            links.set_symbol_synthetic_origin(0, transient(index), SymbolId::new(index));
        }
    }
    links.set_symbol_reverse_mapped_links(
        0,
        transient(201),
        None,
        TypeId::new(7),
        TypeId::new(501),
        TypeId::new(8),
    );
    let cold = links.symbol_cold();
    for index in members {
        assert_eq!(*cold.mapped_type.get(transient(index)), Some(mapped));
        assert_eq!(
            *cold.key_type.get(transient(index)),
            Some(TypeId::new(2_000 + index))
        );
        assert_eq!(
            *cold.synthetic_origin.get(transient(index)),
            (index != 70).then(|| SymbolId::new(index))
        );
        assert_eq!(
            links.symbol(transient(index)).name_type,
            Some(TypeId::new(1_000 + index))
        );
    }
    assert_eq!(
        *cold.mapped_type.get(transient(201)),
        Some(TypeId::new(501))
    );
    assert_eq!(*cold.key_type.get(transient(201)), None);
    for index in [0u32, 6, 69, 71, 199, 202] {
        assert_eq!(*cold.mapped_type.get(transient(index)), None);
        assert_eq!(*cold.key_type.get(transient(index)), None);
        assert_eq!(*cold.synthetic_origin.get(transient(index)), None);
    }

    links.update_symbol_mapped_name_and_key(transient(4), TypeId::new(9_000), TypeId::new(9_001));
    assert_eq!(
        *links.symbol_cold().key_type.get(transient(4)),
        Some(TypeId::new(9_001))
    );
    assert_eq!(
        links.symbol(transient(4)).name_type,
        Some(TypeId::new(9_000))
    );
    assert_eq!(
        *links.symbol_cold().key_type.get(transient(5)),
        Some(TypeId::new(2_005))
    );
}

/// The front of final last-assignment positions answers a kept symbol -
/// "never assigned" and a position alike -, keeps one symbol a slot, and a
/// later write of the field for a kept symbol empties its slot.
#[test]
fn the_last_assignment_front_keeps_final_positions_until_the_field_moves() {
    let mut links = LinksTables::default();
    let symbols: Vec<SymbolId> = [3u32, 17, 4_000, tsc_types::TRANSIENT_SYMBOL_BIT | 9]
        .into_iter()
        .map(SymbolId::new)
        .collect();
    for &symbol in &symbols {
        assert_eq!(links.final_last_assignment_pos(symbol), None);
    }
    links.keep_final_last_assignment_pos(symbols[0], None);
    links.keep_final_last_assignment_pos(symbols[1], Some(42));
    links.keep_final_last_assignment_pos(symbols[2], Some(-7));
    links.keep_final_last_assignment_pos(symbols[3], Some(i64::MAX));
    assert_eq!(links.final_last_assignment_pos(symbols[0]), Some(None));
    assert_eq!(links.final_last_assignment_pos(symbols[1]), Some(Some(42)));
    assert_eq!(links.final_last_assignment_pos(symbols[2]), Some(Some(-7)));
    assert_eq!(
        links.final_last_assignment_pos(symbols[3]),
        Some(Some(i64::MAX))
    );

    // The field moves: the kept answer goes, the others stay.
    links.set_symbol_last_assignment_pos(0, symbols[1], Some(50));
    assert_eq!(links.final_last_assignment_pos(symbols[1]), None);
    assert_eq!(links.final_last_assignment_pos(symbols[0]), Some(None));
    assert_eq!(links.final_last_assignment_pos(symbols[2]), Some(Some(-7)));

    // One symbol a slot: a symbol of the same slot replaces the kept one.
    let rival = (0..u32::MAX - 1)
        .map(SymbolId::new)
        .find(|&candidate| {
            candidate != symbols[2]
                && LastAssignmentFront::slot(candidate) == LastAssignmentFront::slot(symbols[2])
        })
        .unwrap();
    links.keep_final_last_assignment_pos(rival, Some(1));
    assert_eq!(links.final_last_assignment_pos(symbols[2]), None);
    assert_eq!(links.final_last_assignment_pos(rival), Some(Some(1)));
    assert!(LastAssignmentFront::slot(rival) < LastAssignmentFront::SLOTS);
}

/// A write past the end of the dense type and transient symbol links grows
/// them one record at a time: the records between read as unset.
#[test]
fn dense_links_grow_to_a_far_write_with_unset_records_between() {
    let mut links = LinksTables::default();
    let far = TypeId::new(40);
    links.set_fresh_type_members(far, LinkSlot::Vacant);
    assert_eq!(links.ty.records.len(), 41);
    links.set_type_members(0, TypeId::new(3), LinkSlot::Vacant);
    assert_eq!(links.ty.records.len(), 41);
    let symbol = SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | 30);
    links.set_symbol_check_flags(0, symbol, tsc_types::CheckFlags::INSTANTIATED);
    assert_eq!(links.transient_symbols.len(), 31);
    for index in 0..30 {
        let between = SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index);
        assert_eq!(links.read_symbol_check_flags(between).bits(), 0, "{index}");
    }
    assert_eq!(
        links.read_symbol_check_flags(symbol),
        tsc_types::CheckFlags::INSTANTIATED
    );
}

/// A node that holds only a resolved symbol keeps it in a four-byte record;
/// writing any other field moves the record to the full pool with its
/// symbol, and the reads answer the same before and after.
#[test]
fn a_symbol_only_node_record_moves_whole_when_another_field_is_written() {
    let mut links = LinksTables::default();
    let node = NodeId::new(70);
    links.set_node_resolved_symbol(0, node, SymbolId::new(7));
    assert!(matches!(links.node.node_entry(node), NodeEntry::Symbol(..)));
    assert_eq!(
        links.read_node(node, |l| l.resolved_symbol.resolved()),
        Some(SymbolId::new(7))
    );
    assert_eq!(links.node(node).resolved_type.resolved(), None);
    assert_eq!(
        links.node(node).resolved_symbol.resolved(),
        Some(SymbolId::new(7))
    );
    // Rewritten in place while it holds only the symbol.
    links.set_node_resolved_symbol(0, node, SymbolId::new(8));
    assert!(matches!(links.node.node_entry(node), NodeEntry::Symbol(..)));
    assert_eq!(
        links.node(node).resolved_symbol.resolved(),
        Some(SymbolId::new(8))
    );
    let records = links.node.len;
    links.set_node_resolved_type(0, node, LinkSlot::Resolved(TypeId::new(3)));
    assert!(matches!(links.node.node_entry(node), NodeEntry::Full(..)));
    assert_eq!(links.node.len, records, "the record moved, none was added");
    assert_eq!(
        links.read_node(node, |l| (
            l.resolved_symbol.resolved(),
            l.resolved_type.resolved()
        )),
        (Some(SymbolId::new(8)), Some(TypeId::new(3)))
    );
    // A full record's symbol is written in the record.
    links.set_node_resolved_symbol(0, node, SymbolId::new(9));
    assert_eq!(
        links.node(node).resolved_symbol.resolved(),
        Some(SymbolId::new(9))
    );
    assert_eq!(
        links.node(node).resolved_type.resolved(),
        Some(TypeId::new(3))
    );
    // The late-bind pair on a node of its own: written, then taken back.
    let late = NodeId::new(71);
    links.set_node_resolved_symbol_late_bind(0, late, SymbolId::new(11));
    assert_eq!(
        links.node(late).resolved_symbol.resolved(),
        Some(SymbolId::new(11))
    );
    links.revert_node_resolved_symbol_late_bind(late);
    assert_eq!(links.node(late).resolved_symbol.resolved(), None);
    assert!(matches!(links.node.node_entry(late), NodeEntry::Symbol(..)));
    links.or_node_check_flags(0, late, tsc_types::NodeCheckFlags::TYPE_CHECKED);
    assert_eq!(links.node(late).resolved_symbol.resolved(), None);
    assert!(links
        .node(late)
        .check_flags
        .contains(tsc_types::NodeCheckFlags::TYPE_CHECKED));
}

/// A flow query keeps a symbol-only record's word as its reference's place:
/// the word reads the current symbol while the record holds only it, and
/// once the record moves to the full pool the word says so and the read goes
/// by node, so a symbol written later in the full record is the one read.
#[test]
fn a_symbol_only_records_word_reads_its_current_symbol_and_follows_a_move() {
    let mut links = LinksTables::default();
    let node = NodeId::new(70);
    links.set_node_resolved_symbol(0, node, SymbolId::new(7));
    let (root, symbol) = links.node_symbol_with_location(node).unwrap();
    assert_eq!(symbol.resolved(), Some(SymbolId::new(7)));
    assert!(root.location().is_none());
    let word = root.symbol_word().expect("a symbol-only record has a word");
    assert_eq!(
        links.node_symbol_at_word(node, word),
        Some(SymbolId::new(7))
    );
    links.set_node_resolved_symbol(0, node, SymbolId::new(8));
    assert_eq!(
        links.node_symbol_at_word(node, word),
        Some(SymbolId::new(8))
    );
    // Another field moves the record; its word now reads as moved.
    links.set_node_resolved_type(0, node, LinkSlot::Resolved(TypeId::new(3)));
    assert!(matches!(links.node.node_entry(node), NodeEntry::Full(..)));
    assert_eq!(
        links.node_symbol_at_word(node, word),
        Some(SymbolId::new(8))
    );
    links.set_node_resolved_symbol(0, node, SymbolId::new(9));
    assert_eq!(
        links.node_symbol_at_word(node, word),
        Some(SymbolId::new(9))
    );
    // A full record's place is its location.
    let (root, _) = links.node_symbol_with_location(node).unwrap();
    assert!(root.location().is_some());
    assert!(root.symbol_word().is_none());
}

/// A region's symbol-only records run out after `REGION_SYMBOL_RECORDS`:
/// the next node of the region keeps its symbol in a full record.
#[test]
fn a_region_past_its_symbol_only_records_keeps_symbols_in_full_records() {
    let mut links = LinksTables::default();
    let base = 5 << REGION_SHIFT;
    for offset in 0..=REGION_SYMBOL_RECORDS {
        let node = NodeId::new((base + offset) as u32);
        links.set_node_resolved_symbol(0, node, SymbolId::new(offset as u32 + 1));
    }
    for offset in 0..=REGION_SYMBOL_RECORDS {
        let node = NodeId::new((base + offset) as u32);
        assert_eq!(
            links.node(node).resolved_symbol.resolved(),
            Some(SymbolId::new(offset as u32 + 1)),
            "{offset}"
        );
        let full = matches!(links.node.node_entry(node), NodeEntry::Full(..));
        assert_eq!(full, offset == REGION_SYMBOL_RECORDS, "{offset}");
    }
    assert_eq!(links.node.len, REGION_SYMBOL_RECORDS + 1);
}

/// A cursor keeps the location of a full record only: an outer cursor that
/// read a symbol-only record still reads the node's fields after an inner
/// write moved the record to the full pool.
#[test]
fn a_cursor_keeps_no_location_of_a_record_that_can_move() {
    let mut links = LinksTables::default();
    let node = NodeId::new(90);
    links.set_node_resolved_symbol(0, node, SymbolId::new(4));
    let mut outer = NodeLinksCursor::new(node);
    assert_eq!(
        outer.read(&links, |l| l.resolved_symbol.resolved()),
        Some(SymbolId::new(4))
    );
    assert!(outer.location.is_none());
    let mut inner = NodeLinksCursor::new(node);
    inner.set_type(&mut links, LinkSlot::Resolved(TypeId::new(6)));
    assert!(inner.location.is_some());
    assert_eq!(
        outer.read(&links, |l| (
            l.resolved_symbol.resolved(),
            l.resolved_type.resolved()
        )),
        (Some(SymbolId::new(4)), Some(TypeId::new(6)))
    );
    // Its location now kept (the record is full and will not move again).
    assert!(outer.location.is_some());
    outer.set_symbol(&mut links, SymbolId::new(5));
    assert_eq!(
        links.node(node).resolved_symbol.resolved(),
        Some(SymbolId::new(5))
    );
    assert_eq!(
        links.node(node).resolved_type.resolved(),
        Some(TypeId::new(6))
    );
}
