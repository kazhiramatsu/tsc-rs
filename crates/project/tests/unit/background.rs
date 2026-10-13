//! tsgo `project/background/queue_test.go`, and the cancellation's sleep.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Cancellation, Queue};

#[test]
fn basic_enqueue() {
    let queue = Arc::new(Queue::default());
    let executed = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&executed);
    queue.enqueue(&Cancellation::default(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    queue.wait();
    assert_eq!(executed.load(Ordering::SeqCst), 1);
    queue.close();
}

#[test]
fn multiple_tasks_execution() {
    let queue = Arc::new(Queue::default());
    let executed = Arc::new(AtomicUsize::new(0));
    for _ in 0..10 {
        let counter = Arc::clone(&executed);
        queue.enqueue(&Cancellation::default(), move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
    }
    queue.wait();
    assert_eq!(executed.load(Ordering::SeqCst), 10);
    queue.close();
}

#[test]
fn nested_enqueue() {
    let queue = Arc::new(Queue::default());
    let executed = Arc::new(Mutex::new(Vec::new()));
    let (parent_queue, parent_log) = (Arc::clone(&queue), Arc::clone(&executed));
    queue.enqueue(&Cancellation::default(), move |cancellation| {
        parent_log.lock().unwrap().push("parent");
        let child_log = Arc::clone(&parent_log);
        parent_queue.enqueue(cancellation, move |_| {
            child_log.lock().unwrap().push("child");
        });
    });
    queue.wait();
    assert_eq!(executed.lock().unwrap().len(), 2);
    queue.close();
}

#[test]
fn a_closed_queue_rejects_new_tasks() {
    let queue = Arc::new(Queue::default());
    queue.close();
    let executed = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&executed);
    queue.enqueue(&Cancellation::default(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    queue.wait();
    assert_eq!(executed.load(Ordering::SeqCst), 0);
}

#[test]
fn close_waits_for_active_tasks() {
    let queue = Arc::new(Queue::default());
    let (started_tx, started) = mpsc::channel();
    let (finish, finish_rx) = mpsc::channel::<()>();
    queue.enqueue(&Cancellation::default(), move |_| {
        started_tx.send(()).unwrap();
        let _ = finish_rx.recv();
    });
    started.recv().unwrap();
    let (closed_tx, closed) = mpsc::channel();
    let closing = Arc::clone(&queue);
    let closer = std::thread::spawn(move || {
        closing.close();
        closed_tx.send(()).unwrap();
    });
    assert!(
        closed.recv_timeout(Duration::from_millis(100)).is_err(),
        "close returned before the active task completed"
    );
    finish.send(()).unwrap();
    closed.recv().unwrap();
    closer.join().unwrap();
}

#[test]
fn a_cancelled_task_does_not_start() {
    let queue = Arc::new(Queue::default());
    let cancellation = Cancellation::default();
    cancellation.cancel();
    let executed = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&executed);
    queue.enqueue(&cancellation, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    queue.wait();
    assert_eq!(executed.load(Ordering::SeqCst), 0);
}

#[test]
fn a_cancellation_ends_a_sleep() {
    let cancellation = Cancellation::default();
    assert!(cancellation.sleep(Duration::ZERO));
    let sleeping = cancellation.clone();
    let start = Instant::now();
    let sleeper = std::thread::spawn(move || sleeping.sleep(Duration::from_secs(60)));
    cancellation.cancel();
    assert!(!sleeper.join().unwrap());
    assert!(start.elapsed() < Duration::from_secs(30));
    assert!(!cancellation.sleep(Duration::from_secs(60)));
}
