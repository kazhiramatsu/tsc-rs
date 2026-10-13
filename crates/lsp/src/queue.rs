//! tsgo `dynamicQueue`: an unbounded first-in first-out queue whose `get`
//! waits for an item. tsgo cancels a waiting call with its context; here the
//! server closes the queue when it stops, which ends every waiting `get` and
//! refuses later `put`s.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, PoisonError};

pub struct DynamicQueue<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
}

struct State<T> {
    items: VecDeque<T>,
    closed: bool,
}

impl<T> Default for DynamicQueue<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                items: VecDeque::new(),
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }
}

impl<T> DynamicQueue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `item`; false once the queue is closed.
    pub fn put(&self, item: T) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return false;
        }
        state.items.push_back(item);
        self.ready.notify_one();
        true
    }

    /// The first item, waiting for one; none once the queue is closed.
    pub fn get(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if state.closed {
                return None;
            }
            if let Some(item) = state.items.pop_front() {
                return Some(item);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Ends the waiting `get`s and refuses later items.
    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        state.items.clear();
        self.ready.notify_all();
    }
}
