use super::*;

fn small_domain() -> IdentityDomain {
    IdentityDomain::with_limits(IdentityLimits {
        node_end: 16,
        node_array_end: 16,
        persistent_symbol_end: 16,
        private_name_serial_end: 17,
        node_array_item_end: 16,
        type_end: 16,
        node_text_end: 16,
    })
    .unwrap()
}

#[test]
fn reclaiming_leases_never_overlap_and_release_only_after_last_clone() {
    let domain = small_domain();
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
    let domain = small_domain();
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
fn domains_of_one_process_never_share_an_identity() {
    let first = IdentityDomain::new();
    let second = IdentityDomain::new();
    let one = first.lease(IdentitySpace::Node, 8).unwrap();
    let other = second.lease(IdentitySpace::Node, 8).unwrap();
    assert!(!one.range().overlaps(other.range()));
    assert!(!first.same_domain(&second));
    assert!(first.owns(&one) && !second.owns(&one));
}

#[test]
fn batch_failure_changes_no_identity_space() {
    let domain = small_domain();
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

#[test]
fn a_lease_is_claimed_once_at_a_time() {
    let domain = IdentityDomain::new();
    let lease = domain.lease(IdentitySpace::Node, 3).unwrap();
    let claim = lease.claim().expect("free lease");
    assert!(lease.claim().is_none());
    assert_eq!(claim.lease().range(), lease.range());
    drop(claim);
    assert!(lease.claim().is_some());
}

#[test]
fn a_watch_sees_a_lease_released_by_its_last_holder() {
    let domain = IdentityDomain::new();
    let lease = domain.lease(IdentitySpace::Node, 3).unwrap();
    let watch = lease.watch();
    assert!(watch.is_held());
    assert!(watch.watches(&lease));
    let other = domain.lease(IdentitySpace::Node, 1).unwrap();
    assert!(!watch.watches(&other));
    let held = lease.clone();
    drop(lease);
    assert!(watch.is_held());
    drop(held);
    assert!(!watch.is_held());
}
