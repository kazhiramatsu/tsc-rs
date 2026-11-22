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
    assert_eq!(std::mem::size_of::<TypeLinks>(), 20);
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
