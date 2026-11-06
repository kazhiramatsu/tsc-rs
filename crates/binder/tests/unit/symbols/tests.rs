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
        .map(|name| arena.alloc(SymbolFlags::NONE, name.clone()))
        .collect();
    let mut table = SymbolTable::default();
    for count in 0..names.len() {
        assert_eq!(table.insert(names[count].clone(), ids[count]), None);
        for (index, name) in names.iter().enumerate() {
            let expected = (index <= count).then_some(&ids[index]);
            assert_eq!(
                table.get(name.clone()),
                expected,
                "{count} entries, {name:?}"
            );
        }
        assert_eq!(table.len(), count + 1);
    }
    // A replacement keeps its position, scanned or indexed alike.
    let mut small = SymbolTable::default();
    for count in 0..3 {
        small.insert(names[count].clone(), ids[count]);
    }
    assert_eq!(small.insert(names[1].clone(), ids[19]), Some(ids[1]));
    assert_eq!(
        small.get_full(names[1].clone()),
        Some((1, &names[1], &ids[19]))
    );
    assert_eq!(table.insert(names[1].clone(), ids[19]), Some(ids[1]));
    assert_eq!(
        table.get_full(names[1].clone()),
        Some((1, &names[1], &ids[19]))
    );
    // Removal keeps the others in order on both sides.
    assert_eq!(small.shift_remove(names[0].clone()), Some(ids[0]));
    assert_eq!(
        small.keys().cloned().collect::<Vec<_>>(),
        [names[1].clone(), names[2].clone()]
    );
    assert_eq!(table.shift_remove(names[0].clone()), Some(ids[0]));
    assert_eq!(
        table.get_full(names[2].clone()),
        Some((1, &names[2], &ids[2]))
    );
    assert_eq!(table.get(names[0].clone()), None);
    assert_eq!(table.len(), 19);
    // An indexed and a scanned map with the same entries are equal.
    let mut scanned = SymbolTable::default();
    let mut indexed = SymbolTable::default();
    for count in 0..5 {
        scanned.insert(names[count].clone(), ids[count]);
    }
    for count in 0..12 {
        indexed.insert(names[count].clone(), ids[count]);
    }
    for count in 5..12 {
        indexed.shift_remove(names[count].clone());
    }
    assert_eq!(scanned, indexed);
}
