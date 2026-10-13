//! tsgo `project/background` (queue.go): work run off the request path,
//! each piece on its own thread, waited for together; and the cancellation
//! that stands for tsgo's cancelled contexts.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A cancellation shared with background work (tsgo's cancellable
/// context).
#[derive(Clone, Debug, Default)]
pub(crate) struct Cancellation(Arc<(Mutex<bool>, Condvar)>);

impl Cancellation {
    pub(crate) fn cancel(&self) {
        *lock(&self.0 .0) = true;
        self.0 .1.notify_all();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *lock(&self.0 .0)
    }

    /// tsgo's `select` on `time.After(delay)` and `ctx.Done()`: whether the
    /// delay passed before a cancellation.
    pub(crate) fn sleep(&self, delay: Duration) -> bool {
        let cancelled = lock(&self.0 .0);
        let (cancelled, _) = self
            .0
             .1
            .wait_timeout_while(cancelled, delay, |cancelled| !*cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        !*cancelled
    }
}

#[derive(Default)]
struct QueueState {
    running: usize,
    closed: bool,
}

/// tsgo `background.Queue`.
#[derive(Default)]
pub(crate) struct Queue {
    state: Mutex<QueueState>,
    idle: Condvar,
}

/// One piece of work's place in the queue, given back when it ends (even
/// by a panic).
struct Running(Arc<Queue>);

impl Drop for Running {
    fn drop(&mut self) {
        let mut state = lock(&self.0.state);
        state.running -= 1;
        if state.running == 0 {
            self.0.idle.notify_all();
        }
    }
}

impl Queue {
    /// tsgo `Enqueue`: `work` runs on its own thread unless the queue is
    /// closed or `cancellation` is cancelled before it starts.
    pub(crate) fn enqueue(
        self: &Arc<Self>,
        cancellation: &Cancellation,
        work: impl FnOnce(&Cancellation) + Send + 'static,
    ) {
        {
            let mut state = lock(&self.state);
            if state.closed || cancellation.is_cancelled() {
                return;
            }
            state.running += 1;
        }
        let running = Running(Arc::clone(self));
        let cancellation = cancellation.clone();
        std::thread::spawn(move || {
            let _running = running;
            if !cancellation.is_cancelled() {
                work(&cancellation);
            }
        });
    }

    /// tsgo `Wait`: the work enqueued so far, and what it enqueued, ended.
    pub(crate) fn wait(&self) {
        let mut state = lock(&self.state);
        while state.running > 0 {
            state = self
                .idle
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// tsgo `Close`: no more work, and the running work ended.
    pub(crate) fn close(&self) {
        lock(&self.state).closed = true;
        self.wait();
    }
}

#[cfg(test)]
#[path = "../tests/unit/background.rs"]
mod tests;
