//! tsgo `lsp/dynamic_queue_test.go`. tsgo cancels a waiting call with its
//! context; the server here closes the queue when it stops.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tsc_lsp::queue::DynamicQueue;

/// tsgo `TestDynamicQueueFIFO`.
#[test]
fn first_in_first_out() {
    let queue = DynamicQueue::new();
    for item in 0..1000 {
        assert!(queue.put(item));
    }
    for item in 0..1000 {
        assert_eq!(queue.get(), Some(item));
    }
}

/// tsgo `TestDynamicQueueGetCancellation`: a waiting `get` ends.
#[test]
fn closing_ends_a_waiting_get() {
    let queue = Arc::new(DynamicQueue::<i32>::new());
    let waiting = {
        let queue = Arc::clone(&queue);
        thread::spawn(move || queue.get())
    };
    thread::sleep(Duration::from_millis(20));
    queue.close();
    assert_eq!(waiting.join().unwrap(), None);
}

/// tsgo `TestDynamicQueuePutCancellationWhileStateUnavailable`: a stopped
/// queue takes no items, and a waiting `get` gets the item put after it.
#[test]
fn put_after_close_fails_and_get_waits_for_put() {
    let queue = Arc::new(DynamicQueue::new());
    let waiting = {
        let queue = Arc::clone(&queue);
        thread::spawn(move || queue.get())
    };
    thread::sleep(Duration::from_millis(20));
    assert!(queue.put(2));
    assert_eq!(waiting.join().unwrap(), Some(2));
    queue.close();
    assert!(!queue.put(3));
    assert_eq!(queue.get(), None);
}
