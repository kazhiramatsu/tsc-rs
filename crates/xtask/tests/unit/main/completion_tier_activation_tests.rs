use super::*;

#[test]
fn inactive_or_incoherent_artifacts_keep_the_completion_row_red() {
    let probe = tier_1_through_3_activation_probe(Err(
        "oracle-input comparators remain explicit \"absent\" markers".to_owned(),
    ));

    assert!(!probe.ready);
    assert!(probe.detail.contains("activation proof failed"));
    assert!(probe.detail.contains("comparators remain explicit"));
}

#[test]
fn exact_artifact_activation_is_reported_with_derived_counts() {
    let probe =
        tier_1_through_3_activation_probe(Ok(tsc_conformance::ratchet::Tier1Through3Activation {
            t1_matched: 11,
            t2_matched: 10,
            t3_matched: 9,
            total: 12,
        }));

    assert!(probe.ready);
    assert!(probe.detail.contains("oracle-input comparators active"));
    assert!(probe.detail.contains("T1=11/12 T2=10/12 T3=9/12"));
}

#[test]
fn active_but_empty_accepted_tiers_keep_the_completion_row_red() {
    let probe =
        tier_1_through_3_activation_probe(Ok(tsc_conformance::ratchet::Tier1Through3Activation {
            t1_matched: 1,
            t2_matched: 0,
            t3_matched: 0,
            total: 12,
        }));

    assert!(!probe.ready);
    assert!(probe.detail.contains("T2=0/12 T3=0/12"));
    assert!(probe.detail.contains("must be nonzero"));
}

#[test]
fn t4_activation_failure_keeps_the_completion_row_red() {
    let probe = t4_activation_probe(Err(
        "render_driver_sha256 drift against the current producer".to_owned(),
    ));

    assert!(!probe.ready);
    assert!(probe.detail.contains("T4 activation proof failed"));
    assert!(probe.detail.contains("render_driver_sha256 drift"));
}

#[test]
fn t4_activation_requires_every_nonempty_case() {
    let complete = t4_activation_probe(Ok(tsc_conformance::ratchet::T4Activation {
        matched_cases: 12,
        total_cases: 12,
    }));
    assert!(complete.ready);
    assert!(complete.detail.contains("accepted cases=12/12"));

    for activation in [
        tsc_conformance::ratchet::T4Activation {
            matched_cases: 11,
            total_cases: 12,
        },
        tsc_conformance::ratchet::T4Activation {
            matched_cases: 0,
            total_cases: 0,
        },
    ] {
        let probe = t4_activation_probe(Ok(activation));
        assert!(!probe.ready);
        assert!(probe
            .detail
            .contains("accepted count must equal a nonzero total"));
    }
}
