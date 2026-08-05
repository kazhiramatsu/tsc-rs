use super::{select_invariant_pipeline_workers, PostJobsInvariant, POST_JOBS_INVARIANTS};

#[test]
fn defaults_to_two_workers_without_oversubscribing_one_core() {
    assert_eq!(select_invariant_pipeline_workers(None, 8, true).unwrap(), 2);
    assert_eq!(select_invariant_pipeline_workers(None, 1, true).unwrap(), 1);
    assert_eq!(
        select_invariant_pipeline_workers(Some("2"), 1, true).unwrap(),
        1
    );
}

#[test]
fn accepts_only_the_bounded_worker_policy() {
    assert_eq!(
        select_invariant_pipeline_workers(Some("1"), 8, true).unwrap(),
        1
    );
    assert_eq!(
        select_invariant_pipeline_workers(Some("2"), 8, true).unwrap(),
        2
    );
    for invalid in ["0", "3", "many"] {
        assert!(select_invariant_pipeline_workers(Some(invalid), 8, true).is_err());
    }
    assert!(select_invariant_pipeline_workers(None, 0, true).is_err());
}

#[test]
fn cache_off_forces_the_serial_caller_thread_policy() {
    assert_eq!(
        select_invariant_pipeline_workers(Some("2"), 8, false).unwrap(),
        1
    );
}

#[test]
fn independent_suites_are_paired_into_the_reviewed_two_lanes() {
    let lane_zero = POST_JOBS_INVARIANTS
        .iter()
        .copied()
        .step_by(2)
        .collect::<Vec<_>>();
    let lane_one = POST_JOBS_INVARIANTS
        .iter()
        .copied()
        .skip(1)
        .step_by(2)
        .collect::<Vec<_>>();
    assert_eq!(
        lane_zero,
        [
            PostJobsInvariant::Encodings,
            PostJobsInvariant::MatrixIndependence,
        ]
    );
    assert_eq!(
        lane_one,
        [
            PostJobsInvariant::Idempotence,
            PostJobsInvariant::UnsupportedUnwind,
        ]
    );
}
