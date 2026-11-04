//! A tree relocated into an identity domain equals a direct parse at the
//! bases it got.
//!
//! This binary holds this one test, so that it runs in a process of its own.
//! Identities are process-wide (every `IdentityDomain::new` allocates from
//! one state), and a test running beside it would lease from the same spaces
//! and move where the tree's literal texts land, while a direct parse always
//! puts them at the start of their space (`NodeArena` equality compares the
//! texts' base and the nodes' text identities).

use tsc_syntax::{parse_source_file, ParseOptions};
use tsc_types::{IdentityDomain, IdentitySpace};

#[test]
fn generated_relocation_matches_a_direct_forced_nonzero_parse() {
    let text = r#"
/** @typedef {{ value: string }} Alias */
export class Box { #value = 1; method<T>(arg: T) { return [arg, this.#value]; } }
"#;
    let mut local = parse_source_file("forced.ts", text, ParseOptions::default(), None);
    let domain = IdentityDomain::new();
    // Leases ahead of the tree keep its node and array bases away from the
    // start of their spaces.
    let node_gap = domain.lease(IdentitySpace::Node, 11).unwrap();
    let array_gap = domain.lease(IdentitySpace::NodeArray, 7).unwrap();

    local.relocate_into_identity_domain(&domain).unwrap();
    assert!(local.identity_owned_by(&domain));
    let leases = local.arena.identity_leases().unwrap();
    assert_eq!(leases.nodes.range().start(), local.arena.node_base());
    assert_eq!(leases.arrays.range().start(), local.arena.array_base());
    assert_eq!(
        leases.texts.range().start(),
        0,
        "the texts start their space only in a process of the test's own"
    );
    let nodes = leases.nodes.watch();

    let direct = parse_source_file(
        "forced.ts",
        text,
        ParseOptions {
            node_id_base: local.arena.node_base(),
            node_array_id_base: local.arena.array_base(),
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(local, direct);
    assert!(local
        .arena
        .node_ids()
        .all(|id| local.arena.contains_node(id)));

    drop(local);
    assert!(!nodes.is_held());
    let gap = node_gap.watch();
    drop(node_gap);
    drop(array_gap);
    assert!(!gap.is_held());
}
