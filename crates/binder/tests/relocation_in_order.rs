//! The checker binds Program sources locally on worker threads and relocates
//! them in Program order; that must publish exactly the symbol and
//! private-name-serial identities of the sequential in-domain bind.
//!
//! Identities are process-wide: every `IdentityDomain::new` allocates from
//! one state. The test asserts that three relocations in a row take gap-free
//! ranges, which holds only while nothing else in the process takes a lease
//! between them — so it is the only test of this binary. (In the binder's
//! unit-test binary a test running beside it took three symbol identities
//! between two of the relocations: hosted run 43260459906.)

use tsc_binder::{BindData, Binder};
use tsc_syntax::{ParseOptions, SourceFile};
use tsc_types::IdentityDomain;

fn parse_in_domain(name: &str, text: &str, domain: &IdentityDomain) -> SourceFile {
    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        name,
        tsc_diagnostics::TextSnapshot::new(
            text.to_owned(),
            tsc_diagnostics::DocumentVersion::default(),
        ),
        ParseOptions::default(),
        None,
        domain,
    )
    .unwrap()
}

#[test]
fn local_bind_with_in_order_relocation_matches_direct_binding_at_the_same_bases() {
    let texts = [
        "export class Box { #value = 1; copy(other: Box) { return other.#value + this.#value; } }\nexport const boxed = new Box();\n",
        "class Other { #x = 2; #y = 3; read() { return this.#x + this.#y; } }\nlet other = new Other();\nfunction f(a: number) { let b = a; return b; }\n",
        "export {};\nlet unused = 1;\nnamespace N { export const n = 1; }\n",
    ];

    let domain = IdentityDomain::new();
    let mut sources: Vec<SourceFile> = texts
        .iter()
        .enumerate()
        .map(|(index, text)| parse_in_domain(&format!("/f{index}.ts"), text, &domain))
        .collect();
    let mut relocated: Vec<Binder<'_>> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter_mut()
            .rev()
            .map(|source| scope.spawn(|| Binder::bind_local(source)))
            .collect();
        let mut bound: Vec<Binder<'_>> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        bound.reverse();
        bound
    });
    for binder in &mut relocated {
        assert!(binder.symbol_identity_lease().is_none());
        binder.relocate_into_identity_domain(&domain).unwrap();
        assert!(binder.identity_owned_by(&domain));
    }

    // The three files occupy strictly increasing, gap-free identity ranges.
    let mut previous_symbol_end = None;
    let mut previous_serial_end = None;
    for binder in &relocated {
        let symbols = binder.symbol_identity_lease().unwrap().range();
        let serials = binder.private_name_serial_lease().unwrap().range();
        if let Some(end) = previous_symbol_end {
            assert_eq!(symbols.start(), end);
        }
        if let Some(end) = previous_serial_end {
            assert_eq!(serials.start(), end);
        }
        previous_symbol_end = Some(symbols.end());
        previous_serial_end = Some(serials.end());
    }

    // Each relocated bind equals a bind constructed directly at its bases.
    // The relocated binders go first: a published tree is bound once at a
    // time.
    let expected: Vec<(u32, u32, BindData, Vec<i32>)> = relocated
        .iter()
        .map(|binder| {
            (
                binder.symbol_identity_lease().unwrap().range().start(),
                binder.private_name_serial_lease().unwrap().range().start(),
                BindData::from_binder(binder),
                binder
                    .source
                    .arena
                    .nodes()
                    .iter()
                    .map(|node| node.flags)
                    .collect(),
            )
        })
        .collect();
    drop(relocated);
    for (source, (symbol_base, serial_base, expected, expected_flags)) in
        sources.iter_mut().zip(expected)
    {
        let mut direct = Binder::with_bases(source, serial_base, symbol_base);
        direct.bind_source_file();
        let direct_flags: Vec<i32> = direct
            .source
            .arena
            .nodes()
            .iter()
            .map(|node| node.flags)
            .collect();
        let direct = BindData::from_binder(&direct);
        assert_eq!(
            expected.private_name_serial_base,
            direct.private_name_serial_base
        );
        assert_eq!(expected.next_symbol_id, direct.next_symbol_id);
        assert_eq!(expected.symbols, direct.symbols);
        assert_eq!(expected.node_symbol, direct.node_symbol);
        assert_eq!(expected.node_local_symbol, direct.node_local_symbol);
        assert_eq!(expected.locals, direct.locals);
        assert_eq!(expected.classifiable_names, direct.classifiable_names);
        assert_eq!(expected.assigned_symbol_ids, direct.assigned_symbol_ids);
        assert_eq!(expected.bind_diagnostics, direct.bind_diagnostics);
        assert_eq!(expected.node_flow, direct.node_flow);
        assert_eq!(expected_flags, direct_flags);
    }
}
