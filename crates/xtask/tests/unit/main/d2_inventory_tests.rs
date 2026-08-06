use super::*;

#[test]
fn ledger_reads_every_port_block_on_one_rust_function() {
    // Keep complete ledger markers out of this Rust source's own
    // line-oriented ledger scan.
    let source = [
        concat!("/// tsc-", "port: firstOwner @6.0.3\n"),
        concat!("/// tsc-", "hash: aaa\n"),
        concat!("/// tsc-", "span: _tsc.js:10-12\n"),
        "///\n",
        concat!("/// tsc-", "port: secondOwner @6.0.3\n"),
        concat!("/// tsc-", "hash: bbb\n"),
        concat!("/// tsc-", "span: _tsc.js:20-24\n"),
        "pub(crate) fn combined_owner() {}\n",
    ]
    .concat();
    let entries = parse_ledger_entries_in_file(Path::new("combined.rs"), &source).expect("ledger");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].rust_fn, "combined_owner");
    assert_eq!(entries[0].port_name, "firstOwner");
    assert_eq!((entries[0].span_start, entries[0].span_end), (10, 12));
    assert_eq!(entries[0].hash, "aaa");
    assert_eq!(entries[1].rust_fn, "combined_owner");
    assert_eq!(entries[1].port_name, "secondOwner");
    assert_eq!((entries[1].span_start, entries[1].span_end), (20, 24));
    assert_eq!(entries[1].hash, "bbb");
}

#[test]
fn committed_schema_two_inventory_has_exact_graph_and_ledger_join() {
    let workspace = find_workspace_root().expect("workspace");
    let inventory: M8EmitterInventory =
        read_json(&workspace.join("m8-emitter-inventory.json")).expect("inventory");
    validate_d2_inventory(&inventory).expect("schema-2 graph");
    let declaration = inventory
        .functions
        .iter()
        .find(|function| {
            function.source_range.start.line == 64103 && function.source_range.end.line == 64114
        })
        .expect("getBestMatchIndexedAccessTypeOrUndefined declaration");
    assert_eq!(declaration.name, "getBestMatchIndexedAccessTypeOrUndefined");
    let ledger = collect_ledger_entries(&workspace).expect("ledger");
    let joins = exact_ledger_matches(declaration, &ledger);
    assert_eq!(joins.len(), 1);
    assert_eq!(joins[0].rust_fn, "member_elaboration_target_type");
}
