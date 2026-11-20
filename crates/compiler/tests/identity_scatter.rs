//! The identity scatter's own check (`tsc_types::identity`, cargo feature
//! `identity-scatter`). `scripts/identity_scatter.py` builds the suites with
//! the feature and runs them without and with `TSRS_IDENTITY_SCATTER`; this
//! test fails in a run that is not in the state the script judges it in, so
//! a suite cannot pass "under the scatter" in a run that did not scatter.

use tsc_types::{IdentityDomain, IdentityLimits, IdentitySpace};

#[test]
fn ranges_follow_the_requests_unless_the_process_scatters() {
    let scatters = tsc_types::identity::scatters();
    if let Ok(expected) = std::env::var("TSRS_IDENTITY_SCATTER_EXPECTED") {
        assert_eq!(scatters, expected == "on", "this run's scatter state");
    }
    // A domain of its own: its ranges are no document's.
    let domain = IdentityDomain::with_limits(IdentityLimits::default()).unwrap();
    for space in [IdentitySpace::Node, IdentitySpace::Symbol] {
        let leases: Vec<_> = (0..64).map(|_| domain.lease(space, 16).unwrap()).collect();
        let below_an_earlier_one = leases
            .windows(2)
            .filter(|pair| pair[1].range().start() < pair[0].range().start())
            .count();
        assert_eq!(below_an_earlier_one != 0, scatters, "{space}");
        for (index, lease) in leases.iter().enumerate() {
            for other in &leases[index + 1..] {
                assert!(!lease.range().overlaps(other.range()), "{space}");
            }
        }
    }
    // An order the scatter leaves alone: a checker's types.
    let types: Vec<_> = (0..8)
        .map(|_| domain.lease(IdentitySpace::Type, 16).unwrap())
        .collect();
    assert!(types
        .windows(2)
        .all(|pair| pair[0].range().end() == pair[1].range().start()));
}
