//! tsgo starts a goroutine for each request it handles concurrently. A
//! request thread here has a worker's stack and the per-thread caches of the
//! checker's names, which make one costly to start, so the threads are kept
//! and reused: a request goes to an idle thread, or to a new one when none is
//! idle. Nothing waits for a busy thread, as no goroutine does.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

type Work = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct RequestThreads {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    work: VecDeque<Work>,
    /// The threads waiting for work.
    waiting: usize,
    /// The queued work promised to waiting threads.
    promised: usize,
    stopped: bool,
}

impl RequestThreads {
    /// Run `work` on an idle request thread, or on a new one.
    pub fn run(self: &Arc<Self>, work: Work) -> io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.work.push_back(work);
        if state.waiting > state.promised {
            state.promised += 1;
            self.ready.notify_one();
            return Ok(());
        }
        drop(state);
        let threads = Arc::clone(self);
        std::thread::Builder::new()
            .name("tsc-rs-lsp-request".to_owned())
            .stack_size(tsc_program::WORKER_STACK_BYTES)
            .spawn(move || threads.serve())
            .map(drop)
    }

    /// The idle threads end once the work queued is done.
    pub fn stop(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stopped = true;
        self.ready.notify_all();
    }

    fn serve(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(work) = state.work.pop_front() {
                state.promised = state.promised.saturating_sub(1);
                drop(state);
                work();
                state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            if state.stopped {
                return;
            }
            state.waiting += 1;
            while state.work.is_empty() && !state.stopped {
                state = self
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            state.waiting -= 1;
        }
    }
}
