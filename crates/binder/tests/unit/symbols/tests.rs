use super::*;

#[test]
fn escape_adds_underscore_only_for_double_underscore_prefix() {
    assert_eq!(escape_leading_underscores("__proto__"), "___proto__");
    assert_eq!(escape_leading_underscores("__"), "___");
    assert_eq!(escape_leading_underscores("_x"), "_x");
    assert_eq!(escape_leading_underscores("x"), "x");
    assert_eq!(escape_leading_underscores(""), "");
    // Multi-byte first char must not satisfy the byte checks.
    assert_eq!(escape_leading_underscores("あ__"), "あ__");
}

#[test]
fn unescape_strips_exactly_one_of_three_underscores() {
    assert_eq!(unescape_leading_underscores("___proto__"), "__proto__");
    assert_eq!(unescape_leading_underscores("__x"), "__x");
    assert_eq!(unescape_leading_underscores("___"), "__");
    assert_eq!(unescape_leading_underscores("x"), "x");
}

#[test]
fn user_names_cannot_collide_with_internal_names() {
    // Internal names are inserted verbatim; the user spelling of the
    // same text escapes to a distinct key.
    assert_ne!(
        escape_leading_underscores("__call"),
        InternalSymbolName::CALL
    );
}

#[test]
fn symbol_table_preserves_insertion_order() {
    let mut arena = SymbolArena::default();
    let mut table = SymbolTable::default();
    for name in ["z", "a", "m"] {
        let id = arena.alloc(SymbolFlags::NONE, escape_leading_underscores(name));
        table.insert(escape_leading_underscores(name), id);
    }
    let keys: Vec<&str> = table
        .keys()
        .map(|name| name.as_str().expect("these test names are scalar"))
        .collect();
    assert_eq!(keys, ["z", "a", "m"]);
}

#[test]
fn arena_allocates_sequential_ids() {
    let mut arena = SymbolArena::default();
    let first = arena.alloc(SymbolFlags::NONE, crate::escape_leading_underscores("a"));
    let second = arena.alloc(SymbolFlags::NONE, crate::escape_leading_underscores("b"));
    assert_eq!(first, SymbolId::new(0));
    assert_eq!(second, SymbolId::new(1));
    assert_eq!(arena.symbol(second).escaped_name, "b");
    assert_eq!(arena.len(), 2);
}

#[test]
fn persistent_and_transient_partitions_fail_with_typed_exhaustion() {
    let mut persistent = SymbolArena::with_base(tsc_types::TRANSIENT_SYMBOL_BIT - 1);
    assert_eq!(
        persistent
            .try_alloc(SymbolFlags::NONE, crate::escape_leading_underscores("last"))
            .unwrap(),
        SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT - 1)
    );
    let error = persistent
        .try_alloc(
            SymbolFlags::NONE,
            crate::escape_leading_underscores("overflow"),
        )
        .unwrap_err();
    assert!(!error.transient);
    assert_eq!(error.limit, tsc_types::TRANSIENT_SYMBOL_BIT);

    let mut transient = SymbolArena::with_base(u32::MAX - 1);
    assert_eq!(
        transient
            .try_alloc(
                SymbolFlags::TRANSIENT,
                crate::escape_leading_underscores("last")
            )
            .unwrap(),
        SymbolId::new(u32::MAX - 1)
    );
    let error = transient
        .try_alloc(
            SymbolFlags::TRANSIENT,
            crate::escape_leading_underscores("overflow"),
        )
        .unwrap_err();
    assert!(error.transient);
    assert_eq!(error.limit, u32::MAX);
}

#[test]
fn a_symbol_table_reads_the_same_below_and_above_its_index_threshold() {
    // A small map scans its entries; from nine entries it is indexed. Every
    // read, replacement and removal must agree on both sides.
    let mut arena = SymbolArena::default();
    let names: Vec<EscapedName> = (0..20)
        .map(|index| escape_leading_underscores(&format!("n{index}")))
        .collect();
    let ids: Vec<SymbolId> = names
        .iter()
        .map(|name| arena.alloc(SymbolFlags::NONE, *name))
        .collect();
    let mut table = SymbolTable::default();
    for count in 0..names.len() {
        assert_eq!(table.insert(names[count], ids[count]), None);
        for (index, name) in names.iter().enumerate() {
            let expected = (index <= count).then_some(&ids[index]);
            assert_eq!(table.get(*name), expected, "{count} entries, {name:?}");
        }
        assert_eq!(table.len(), count + 1);
    }
    // A replacement keeps its position, scanned or indexed alike.
    let mut small = SymbolTable::default();
    for count in 0..3 {
        small.insert(names[count], ids[count]);
    }
    assert_eq!(small.insert(names[1], ids[19]), Some(ids[1]));
    assert_eq!(small.get_full(names[1]), Some((1, &names[1], &ids[19])));
    assert_eq!(table.insert(names[1], ids[19]), Some(ids[1]));
    assert_eq!(table.get_full(names[1]), Some((1, &names[1], &ids[19])));
    // Removal keeps the others in order on both sides.
    assert_eq!(small.shift_remove(names[0]), Some(ids[0]));
    assert_eq!(
        small.keys().cloned().collect::<Vec<_>>(),
        [names[1], names[2]]
    );
    assert_eq!(table.shift_remove(names[0]), Some(ids[0]));
    assert_eq!(table.get_full(names[2]), Some((1, &names[2], &ids[2])));
    assert_eq!(table.get(names[0]), None);
    assert_eq!(table.len(), 19);
    // An indexed and a scanned map with the same entries are equal.
    let mut scanned = SymbolTable::default();
    let mut indexed = SymbolTable::default();
    for count in 0..5 {
        scanned.insert(names[count], ids[count]);
    }
    for count in 0..12 {
        indexed.insert(names[count], ids[count]);
    }
    for name in &names[5..12] {
        indexed.shift_remove(*name);
    }
    assert_eq!(scanned, indexed);
}

#[test]
fn a_symbol_table_filter_survives_collisions_cloning_removal_and_clear() {
    let names: Vec<_> = (0..=usize::BITS)
        .map(|index| escape_leading_underscores(&format!("tableFilter{index}")))
        .collect();
    let mut lanes = [None; usize::BITS as usize - 1];
    let (kept, absent) = names
        .iter()
        .copied()
        .find_map(|name| {
            let index = name.index() as usize;
            let lane = (index ^ (index >> 6)) & (usize::BITS as usize - 1);
            lanes[lane.min(usize::BITS as usize - 2)]
                .replace(name)
                .map(|kept| (kept, name))
        })
        .unwrap();
    let extras: Vec<_> = names
        .into_iter()
        .filter(|name| *name != kept && *name != absent)
        .take(3)
        .collect();
    let id = SymbolId::new(0);
    let mut table: SymbolTable = [kept, extras[0], extras[1], extras[2]]
        .into_iter()
        .map(|name| (name, id))
        .collect();

    let mut copy = table.clone();
    assert_eq!(copy.get(kept), Some(&id));
    assert_eq!(copy.get(absent), None);
    assert_eq!(copy.shift_remove(kept), Some(id));
    assert_eq!(copy.get(absent), None);
    assert_eq!(copy.insert(absent, id), None);
    assert_eq!(copy.get_full(absent), Some((3, &absent, &id)));
    assert_eq!(table.get(kept), Some(&id));
    assert_eq!(table.get(absent), None);

    table.clear();
    assert_eq!(table.get(kept), None);
    table.extend(copy);
    assert_eq!(
        table.keys().copied().collect::<Vec<_>>(),
        [extras[0], extras[1], extras[2], absent]
    );
    assert_eq!(table.get(absent), Some(&id));
}

#[test]
fn a_symbol_table_holds_two_entries_in_its_map_and_grows_a_vector_beyond() {
    // Most tables hold one or two entries: those live in the map's own block.
    // A third entry moves them into a vector of four slots that doubles when
    // it is full; reads, order and equality do not depend on where the
    // entries live.
    const MAP: usize = 32;
    const ENTRY: usize = 8;
    let mut arena = SymbolArena::default();
    let names: Vec<EscapedName> = (0..40)
        .map(|index| escape_leading_underscores(&format!("m{index}")))
        .collect();
    let ids: Vec<SymbolId> = names
        .iter()
        .map(|name| arena.alloc(SymbolFlags::NONE, *name))
        .collect();
    let filled = |count: usize| {
        let mut table = SymbolTable::default();
        for index in 0..count {
            table.insert(names[index], ids[index]);
        }
        table
    };
    assert_eq!(SymbolTable::default().heap_bytes(), 0);
    assert_eq!(filled(1).heap_bytes(), MAP);
    assert_eq!(filled(2).heap_bytes(), MAP);
    assert_eq!(filled(3).heap_bytes(), MAP + 4 * ENTRY);
    assert_eq!(filled(4).heap_bytes(), MAP + 4 * ENTRY);
    assert_eq!(filled(5).heap_bytes(), MAP + 8 * ENTRY);
    assert_eq!(filled(8).heap_bytes(), MAP + 8 * ENTRY);
    // Room asked for in advance: none for two entries, the asked size
    // beyond; a table that outgrows it gets the next power of two.
    let mut reserved = SymbolTable::default();
    reserved.reserve(2);
    assert_eq!(reserved.heap_bytes(), MAP);
    reserved.reserve(5);
    assert_eq!(reserved.heap_bytes(), MAP + 5 * ENTRY);
    for index in 0..5 {
        reserved.insert(names[index], ids[index]);
    }
    assert_eq!(reserved.heap_bytes(), MAP + 5 * ENTRY);
    reserved.insert(names[5], ids[5]);
    assert_eq!(reserved.heap_bytes(), MAP + 8 * ENTRY);
    // An indexed table (nine entries or more) that fills a reserved size
    // grows its vector while its index already names the new entry: at
    // every reserved size the index is full at a different count.
    for room in 9..24 {
        let mut exact = SymbolTable::default();
        exact.reserve(room);
        assert_eq!(exact.heap_bytes(), MAP + room * ENTRY);
        for index in 0..names.len() {
            assert_eq!(exact.insert(names[index], ids[index]), None);
            assert_eq!(exact.len(), index + 1);
        }
        for index in 0..names.len() {
            assert_eq!(
                exact.get_full(names[index]),
                Some((index, &names[index], &ids[index]))
            );
        }
    }
    // The same entries read the same from the map and from a vector.
    for count in 0..=6 {
        let table = filled(count);
        assert_eq!(table.len(), count);
        assert_eq!(table.is_empty(), count == 0);
        assert_eq!(
            table
                .iter()
                .map(|(name, id)| (*name, *id))
                .collect::<Vec<_>>(),
            names[..count]
                .iter()
                .copied()
                .zip(ids[..count].iter().copied())
                .collect::<Vec<_>>()
        );
        assert_eq!(table.keys().copied().collect::<Vec<_>>(), names[..count]);
        assert_eq!(table.values().copied().collect::<Vec<_>>(), ids[..count]);
        for index in 0..9 {
            assert_eq!(
                table.get(names[index]),
                (index < count).then_some(&ids[index])
            );
            assert_eq!(
                table.get_index(index),
                (index < count).then_some((&names[index], &ids[index]))
            );
        }
        assert_eq!(table.clone(), table);
        assert_eq!(
            table.clone().into_iter().collect::<Vec<_>>(),
            names[..count]
                .iter()
                .copied()
                .zip(ids[..count].iter().copied())
                .collect::<Vec<_>>()
        );
        assert_eq!(table.iter().collect::<SymbolTableEntries>().len(), count);
    }
    // A copy holds what it needs: two entries in its map, three in a vector
    // of three.
    assert_eq!(filled(2).clone().heap_bytes(), MAP);
    assert_eq!(filled(3).clone().heap_bytes(), MAP + 3 * ENTRY);
    // A table that shrinks back to two entries reads as one that never grew.
    let mut shrunk = filled(3);
    assert_eq!(shrunk.shift_remove(names[1]), Some(ids[1]));
    assert_eq!(
        shrunk.keys().copied().collect::<Vec<_>>(),
        [names[0], names[2]]
    );
    let mut same = SymbolTable::default();
    same.insert(names[0], ids[0]);
    same.insert(names[2], ids[2]);
    assert_eq!(shrunk, same);
    assert_eq!(shrunk.insert(names[2], ids[8]), Some(ids[2]));
    assert_eq!(shrunk.get_full(names[2]), Some((1, &names[2], &ids[8])));
    // Values can be rewritten in place in both forms.
    for count in [2, 5] {
        let mut table = filled(count);
        for id in table.values_mut() {
            *id = ids[8];
        }
        assert!(table.values().all(|id| *id == ids[8]));
        *table.get_mut(names[0]).unwrap() = ids[0];
        assert_eq!(table[names[0]], ids[0]);
        table.clear();
        assert_eq!(table.heap_bytes(), 0);
        assert!(table.is_empty());
    }
    // Collected and extended tables take the same forms.
    let collected: SymbolTable = names[..2]
        .iter()
        .copied()
        .zip(ids.iter().copied())
        .collect();
    assert_eq!(collected.heap_bytes(), MAP);
    assert_eq!(collected, filled(2));
    let mut extended = filled(1);
    extended.extend(names[1..7].iter().copied().zip(ids[1..7].iter().copied()));
    assert_eq!(extended, filled(7));
}

type SymbolTableEntries<'a> = Vec<(&'a EscapedName, &'a SymbolId)>;

#[test]
fn declaration_singletons_stay_in_the_header_until_they_grow() {
    let mut pool = DeclarationPool::with_chunk_capacity(3);
    let node = NodeId::new(17);
    let first = pool.append(Declarations::new(), node);
    let copied = first;
    let allocated = pool.alloc(&[node]);
    assert_eq!(first, [node]);
    assert_eq!(allocated, first);
    assert_eq!(pool.live_bytes(), 0);

    let pair = pool.append(first, NodeId::new(18));
    let triple = pool.append(pair, NodeId::new(19));
    let four = pool.append(triple, NodeId::new(20));
    assert_eq!(copied, [node]);
    assert_eq!(allocated, [node]);
    assert_eq!(pair, [node, NodeId::new(18)]);
    assert_eq!(triple, [node, NodeId::new(18), NodeId::new(19)]);
    assert_eq!(
        four,
        [node, NodeId::new(18), NodeId::new(19), NodeId::new(20)]
    );
    assert!(pool.live_bytes() > 0);

    // Extending a shared singleton must not change either prior header.
    let extended = pool.extend(copied, &[NodeId::new(21), NodeId::new(22)]);
    assert_eq!(extended, [node, NodeId::new(21), NodeId::new(22)]);
    assert_eq!(copied, [node]);
    assert_eq!(first, [node]);
    assert_eq!(pool.extend(copied, &[]), copied);
}

#[test]
fn an_inline_declaration_does_not_displace_the_last_pooled_list() {
    let mut pool = DeclarationPool::with_chunk_capacity(8);
    let pair = pool.alloc(&[NodeId::new(1), NodeId::new(2)]);
    let first = pool.append(Declarations::new(), NodeId::new(7));
    let before = pool.live_bytes();
    let triple = pool.append(pair, NodeId::new(3));
    assert_eq!(pool.live_bytes(), before + std::mem::size_of::<NodeId>());
    assert_eq!(pair, [NodeId::new(1), NodeId::new(2)]);
    assert_eq!(triple, [NodeId::new(1), NodeId::new(2), NodeId::new(3)]);
    assert_eq!(first, [NodeId::new(7)]);
}

#[test]
fn owned_declaration_iteration_agrees_with_slices_in_both_directions() {
    fn fused<I: std::iter::FusedIterator + ExactSizeIterator + DoubleEndedIterator>(_: &I) {}

    let mut pool = DeclarationPool::with_chunk_capacity(3);
    for count in 0..7 {
        let nodes: Vec<_> = (0..count).map(NodeId::new).collect();
        let list = pool.alloc(&nodes);
        let mut actual = list.into_iter();
        fused(&actual);
        let mut expected = nodes.iter().copied();
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual.size_hint(), expected.size_hint());
        assert_eq!(actual.clone().count(), expected.clone().count());
        assert_eq!(actual.clone().last(), expected.clone().last());
        while let Some(first) = expected.next() {
            assert_eq!(actual.next(), Some(first));
            assert_eq!(actual.next_back(), expected.next_back());
            assert_eq!(actual.len(), expected.len());
        }
        assert_eq!(actual.next(), None);
        assert_eq!(actual.next_back(), None);
        for skip in 0..9 {
            let mut actual = list.into_iter();
            let mut expected = nodes.iter().copied();
            assert_eq!(actual.nth(skip), expected.nth(skip));
            assert_eq!(actual.collect::<Vec<_>>(), expected.collect::<Vec<_>>());
            let mut actual = list.into_iter();
            let mut expected = nodes.iter().copied();
            assert_eq!(actual.nth_back(skip), expected.nth_back(skip));
            assert_eq!(actual.collect::<Vec<_>>(), expected.collect::<Vec<_>>());
        }
    }
    let mut owned = {
        let temporary = pool.alloc(&[NodeId::new(99)]);
        temporary.into_iter()
    };
    assert_eq!(owned.next_back(), Some(NodeId::new(99)));
    assert_eq!(owned.next(), None);
    assert_eq!(Declarations::default(), []);
}

#[test]
fn cloned_symbol_arenas_own_every_declaration_list() {
    let last_node = NodeId::new(u32::MAX - 1);
    let (cloned, single, multiple) = {
        let mut arena = SymbolArena::default();
        let single = arena.alloc(SymbolFlags::NONE, escape_leading_underscores("single"));
        let multiple = arena.alloc(SymbolFlags::NONE, escape_leading_underscores("multiple"));
        arena.push_declaration(single, last_node);
        arena.push_declaration(multiple, NodeId::new(1));
        arena.push_declaration(multiple, NodeId::new(2));
        (arena.clone(), single, multiple)
    };
    assert_eq!(cloned.symbol(single).declarations(), [last_node]);
    assert_eq!(
        cloned
            .symbol(single)
            .declaration_list()
            .into_iter()
            .collect::<Vec<_>>(),
        [last_node]
    );
    assert_eq!(
        cloned.symbol(multiple).declarations(),
        [NodeId::new(1), NodeId::new(2)]
    );
}
