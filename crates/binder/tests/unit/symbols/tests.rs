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
        let id = arena.alloc(SymbolFlags::NONE, name.to_owned());
        table.insert(name.to_owned(), id);
    }
    let keys: Vec<&str> = table.keys().map(String::as_str).collect();
    assert_eq!(keys, ["z", "a", "m"]);
}

#[test]
fn arena_allocates_sequential_ids() {
    let mut arena = SymbolArena::default();
    let first = arena.alloc(SymbolFlags::NONE, "a".to_owned());
    let second = arena.alloc(SymbolFlags::NONE, "b".to_owned());
    assert_eq!(first, SymbolId(0));
    assert_eq!(second, SymbolId(1));
    assert_eq!(arena.symbol(second).escaped_name, "b");
    assert_eq!(arena.len(), 2);
}

#[test]
fn persistent_and_transient_partitions_fail_with_typed_exhaustion() {
    let mut persistent = SymbolArena::with_base(tsc_types::TRANSIENT_SYMBOL_BIT - 1);
    assert_eq!(
        persistent
            .try_alloc(SymbolFlags::NONE, "last".to_owned())
            .unwrap(),
        SymbolId(tsc_types::TRANSIENT_SYMBOL_BIT - 1)
    );
    let error = persistent
        .try_alloc(SymbolFlags::NONE, "overflow".to_owned())
        .unwrap_err();
    assert!(!error.transient);
    assert_eq!(error.limit, tsc_types::TRANSIENT_SYMBOL_BIT);

    let mut transient = SymbolArena::with_base(u32::MAX - 1);
    assert_eq!(
        transient
            .try_alloc(SymbolFlags::TRANSIENT, "last".to_owned())
            .unwrap(),
        SymbolId(u32::MAX - 1)
    );
    let error = transient
        .try_alloc(SymbolFlags::TRANSIENT, "overflow".to_owned())
        .unwrap_err();
    assert!(error.transient);
    assert_eq!(error.limit, u32::MAX);
}
