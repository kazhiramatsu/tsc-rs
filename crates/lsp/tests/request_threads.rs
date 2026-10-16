//! The threads the server runs its concurrent requests on (tsgo's
//! goroutines): kept and reused, and never making work wait for a busy one.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use tsc_lsp::request_threads::RequestThreads;

/// Work submitted while another is running starts at once: the first waits
/// for the second's signal, which would never come if they ran in turn.
#[test]
fn work_never_waits_for_a_busy_thread() {
    let threads = Arc::new(RequestThreads::default());
    let (signal, wait) = mpsc::channel::<()>();
    let (done, finished) = mpsc::channel::<&'static str>();
    let first_done = done.clone();
    threads
        .run(Box::new(move || {
            wait.recv_timeout(Duration::from_secs(10))
                .expect("the second work's signal");
            first_done.send("first").expect("report");
        }))
        .expect("a thread");
    threads
        .run(Box::new(move || {
            signal.send(()).expect("signal");
            done.send("second").expect("report");
        }))
        .expect("a thread");
    let mut order = vec![
        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("a work"),
        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("a work"),
    ];
    order.sort_unstable();
    assert_eq!(order, ["first", "second"]);
    threads.stop();
}

/// An idle thread takes the next work: two works submitted one after the
/// other run on one thread.
#[test]
fn an_idle_thread_takes_the_next_work() {
    let threads = Arc::new(RequestThreads::default());
    let mut ran_on = Vec::new();
    for _ in 0..2 {
        let (report, reports) = mpsc::channel();
        threads
            .run(Box::new(move || {
                report.send(std::thread::current().id()).expect("report");
            }))
            .expect("a thread");
        ran_on.push(
            reports
                .recv_timeout(Duration::from_secs(10))
                .expect("a work"),
        );
        // Let the thread return to waiting before the next work.
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(ran_on[0], ran_on[1], "the idle thread ran the next work");
    threads.stop();
}
