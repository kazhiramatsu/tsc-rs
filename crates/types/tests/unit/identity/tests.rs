use super::*;

fn small_domain(policy: IdentityAllocationPolicy) -> IdentityDomain {
    IdentityDomain::with_limits(
        policy,
        IdentityLimits {
            node_end: 16,
            node_array_end: 16,
            persistent_symbol_end: 16,
            private_name_serial_end: 17,
        },
    )
    .unwrap()
}

#[test]
fn reclaiming_leases_never_overlap_and_release_only_after_last_clone() {
    let domain = small_domain(IdentityAllocationPolicy::Reclaiming);
    let first = domain.lease(IdentitySpace::Node, 5).unwrap();
    let retained = first.clone();
    let second = domain.lease(IdentitySpace::Node, 4).unwrap();
    assert_eq!(first.range(), IdentityRange::new(0, 5));
    assert_eq!(second.range(), IdentityRange::new(5, 9));
    assert!(!first.range().overlaps(second.range()));

    drop(first);
    let third = domain.lease(IdentitySpace::Node, 5).unwrap();
    assert_eq!(third.range(), IdentityRange::new(9, 14));
    drop(retained);
    let recycled = domain.lease(IdentitySpace::Node, 5).unwrap();
    assert_eq!(recycled.range(), IdentityRange::new(0, 5));
}

#[test]
fn exhaustion_is_typed_and_never_wraps() {
    let domain = small_domain(IdentityAllocationPolicy::Reclaiming);
    let _full = domain.lease(IdentitySpace::Symbol, 16).unwrap();
    assert_eq!(
        domain.lease(IdentitySpace::Symbol, 1).unwrap_err(),
        IdentityError::Exhausted {
            space: IdentitySpace::Symbol,
            requested: 1,
            limit: 16,
        }
    );
}

#[test]
fn provisional_seal_is_atomic_and_cancel_reopens_every_space() {
    let domain = small_domain(IdentityAllocationPolicy::EphemeralBump);
    {
        let reservation = domain
            .reserve_provisional(&[IdentitySpace::Node, IdentitySpace::NodeArray])
            .unwrap();
        assert_eq!(reservation.base(IdentitySpace::Node).unwrap(), 0);
        assert!(matches!(
            domain.lease(IdentitySpace::Node, 1),
            Err(IdentityError::ProvisionalAllocationActive(
                IdentitySpace::Node
            ))
        ));
    }
    let reservation = domain
        .reserve_provisional(&[IdentitySpace::Node, IdentitySpace::NodeArray])
        .unwrap();
    let leases = reservation
        .seal(&[(IdentitySpace::Node, 3), (IdentitySpace::NodeArray, 2)])
        .unwrap();
    assert_eq!(leases[0].range(), IdentityRange::new(0, 3));
    assert_eq!(leases[1].range(), IdentityRange::new(0, 2));
    let stats = domain.stats().unwrap();
    assert_eq!(stats.space(IdentitySpace::Node).active_ranges, 1);
    assert!(!stats.space(IdentitySpace::Node).provisional);
}

#[test]
fn batch_failure_changes_no_identity_space() {
    let domain = small_domain(IdentityAllocationPolicy::Reclaiming);
    let error = domain
        .lease_batch(&[(IdentitySpace::Node, 3), (IdentitySpace::NodeArray, 17)])
        .unwrap_err();
    assert!(matches!(
        error,
        IdentityError::Exhausted {
            space: IdentitySpace::NodeArray,
            ..
        }
    ));
    let stats = domain.stats().unwrap();
    assert_eq!(stats.space(IdentitySpace::Node).bump, 0);
    assert_eq!(stats.space(IdentitySpace::Node).active_ranges, 0);
}
