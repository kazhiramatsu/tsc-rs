use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;

#[test]
fn consumes_every_result_in_item_order_across_workers() {
    let items: Vec<usize> = (0..257).collect();
    let observed = Mutex::new(Vec::new());
    ordered_for_each(
        &items,
        4,
        |index, item| {
            assert_eq!(index, *item);
            // Vary completion order so the reorder buffer is exercised.
            if index % 3 == 0 {
                std::thread::yield_now();
            }
            index * 2
        },
        |index, result| {
            assert_eq!(result, index * 2);
            observed.lock().unwrap().push(index);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*observed.lock().unwrap(), (0..257).collect::<Vec<_>>());
}

#[test]
fn zero_workers_and_empty_inputs_are_typed() {
    let error = ordered_for_each(&[1], 0, |_, item| *item, |_, _| Ok(())).unwrap_err();
    assert!(error.contains("at least one worker"), "{error}");
    ordered_for_each(
        &Vec::<usize>::new(),
        3,
        |_, item| *item,
        |_, _| panic!("no items to consume"),
    )
    .unwrap();
}

#[test]
fn consume_error_stops_the_stream_and_reports_it() {
    let items: Vec<usize> = (0..64).collect();
    let executed = AtomicUsize::new(0);
    let error = ordered_for_each(
        &items,
        3,
        |_, item| {
            executed.fetch_add(1, Ordering::Relaxed);
            *item
        },
        |index, _| {
            if index == 5 {
                Err("stop at five".to_owned())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error, "stop at five");
    // Workers stop shortly after the consumer drops the channel; they must
    // not have raced through the whole corpus while consumption was halted
    // (bounded retention: channel capacity + in-flight per worker).
    assert!(
        executed.load(Ordering::Relaxed) <= 5 + 1 + 3 * 3,
        "executed {} items after an early stop",
        executed.load(Ordering::Relaxed)
    );
}

#[test]
fn worker_panics_become_typed_errors() {
    let items: Vec<usize> = (0..16).collect();
    let error = ordered_for_each(
        &items,
        2,
        |index, item| {
            if index == 3 {
                panic!("boom at three");
            }
            *item
        },
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("worker panicked"), "{error}");
    assert!(error.contains("boom at three"), "{error}");
}
