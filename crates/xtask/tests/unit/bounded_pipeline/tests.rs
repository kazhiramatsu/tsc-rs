use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use super::{ordered_map, PipelineError};

#[test]
fn skewed_completion_is_returned_in_input_order() {
    let items = [10usize, 20, 30, 40, 50, 60];
    let output = ordered_map(&items, 2, |index, item| {
        if index == 0 {
            std::thread::sleep(Duration::from_millis(40));
        }
        item + index
    })
    .expect("ordered map");

    assert_eq!(output, [10, 21, 32, 43, 54, 65]);
}

#[test]
fn active_mapping_is_bounded_by_worker_count() {
    let worker_count = 3usize;
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let first_wave = Arc::new(Barrier::new(worker_count));
    let items = [0usize; 12];

    let output = ordered_map(&items, worker_count, |index, item| {
        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
        maximum.fetch_max(now, Ordering::SeqCst);
        if index < worker_count {
            first_wave.wait();
        }
        std::thread::yield_now();
        active.fetch_sub(1, Ordering::SeqCst);
        item + index
    })
    .expect("bounded map");

    assert_eq!(output, (0..items.len()).collect::<Vec<_>>());
    assert_eq!(maximum.load(Ordering::SeqCst), worker_count);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn one_worker_maps_every_item_in_order() {
    let items = [3usize, 1, 4, 1, 5];
    let caller = std::thread::current().id();
    let output = ordered_map(&items, 1, |index, item| {
        assert_eq!(std::thread::current().id(), caller);
        item * 10 + index
    })
    .expect("single worker map");

    assert_eq!(output, [30, 11, 42, 13, 54]);
}

#[test]
fn zero_workers_is_rejected() {
    let error =
        ordered_map(&[1usize, 2, 3], 0, |_, item| *item).expect_err("zero workers must fail");

    assert_eq!(error, PipelineError::ZeroWorkers);
}

#[test]
fn worker_panic_is_converted_to_a_typed_error() {
    let error = ordered_map(&[1usize, 2, 3, 4], 2, |index, item| {
        assert_ne!(index, 1, "worker probe");
        *item
    })
    .expect_err("worker panic must fail the pipeline");

    match error {
        PipelineError::WorkerPanicked { worker, message } => {
            assert_eq!(worker, 1);
            assert!(message.contains("worker probe"));
        }
        other => panic!("expected worker panic, got {other:?}"),
    }
}
