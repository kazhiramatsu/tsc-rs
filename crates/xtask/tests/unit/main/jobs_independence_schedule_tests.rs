use super::program_indices_in_job_order;

#[test]
fn every_schedule_preserves_the_original_modulo_traversal() {
    assert_eq!(
        program_indices_in_job_order(8, 3).collect::<Vec<_>>(),
        [0, 3, 6, 1, 4, 7, 2, 5]
    );
    for program_count in [0, 1, 2, 7, 19, 32] {
        for jobs in 1..=16 {
            let expected = (0..jobs)
                .flat_map(|job| (0..program_count).filter(move |index| index % jobs == job))
                .collect::<Vec<_>>();
            assert_eq!(
                program_indices_in_job_order(program_count, jobs).collect::<Vec<_>>(),
                expected,
                "program_count={program_count} jobs={jobs}"
            );
        }
    }
}

#[test]
#[should_panic(expected = "jobs-independence requires at least one job")]
fn zero_jobs_is_rejected() {
    let _ = program_indices_in_job_order(1, 0).count();
}
