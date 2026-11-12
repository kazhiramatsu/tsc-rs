//! A root read is claimed once, by either a reader or its parse worker.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

enum State<T> {
    Unclaimed,
    Reading,
    Ready(T),
    Taken,
    Abandoned,
}

struct Slot<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
}

/// A panic must release a parser waiting on the reader that unwound.
struct Claim<'a, T>(&'a Slot<T>);
impl<T> Drop for Claim<'_, T> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().expect("root read slot");
        if matches!(*state, State::Reading) {
            *state = State::Abandoned;
            self.0.ready.notify_all();
        }
    }
}

pub(crate) struct SourceReads<T> {
    slots: Vec<Slot<T>>,
    next: AtomicUsize,
}

impl<T> SourceReads<T> {
    pub(crate) fn new(count: usize) -> Self {
        Self {
            slots: (0..count)
                .map(|_| Slot {
                    state: Mutex::new(State::Unclaimed),
                    ready: Condvar::new(),
                })
                .collect(),
            next: AtomicUsize::new(0),
        }
    }

    /// Read in the scheduler's seed order, skipping slots already claimed.
    pub(crate) fn prepare(&self, order: &[usize], read: &impl Fn(usize) -> T) {
        while let Some(&index) = order.get(self.next.fetch_add(1, Ordering::Relaxed)) {
            let slot = &self.slots[index];
            let mut state = slot.state.lock().expect("root read slot");
            if !matches!(*state, State::Unclaimed) {
                continue;
            }
            *state = State::Reading;
            drop(state);
            let claim = Claim(slot);
            let result = read(index);
            *slot.state.lock().expect("root read slot") = State::Ready(result);
            slot.ready.notify_one();
            drop(claim);
        }
    }

    pub(crate) fn take(&self, index: usize, read: impl FnOnce() -> T) -> T {
        let slot = &self.slots[index];
        let mut state = slot.state.lock().expect("root read slot");
        loop {
            match &*state {
                State::Unclaimed => {
                    *state = State::Reading;
                    drop(state);
                    let claim = Claim(slot);
                    let result = read();
                    *slot.state.lock().expect("root read slot") = State::Taken;
                    drop(claim);
                    return result;
                }
                State::Reading => state = slot.ready.wait(state).expect("root reader"),
                State::Ready(_) => {
                    let State::Ready(result) = std::mem::replace(&mut *state, State::Taken) else {
                        unreachable!()
                    };
                    return result;
                }
                State::Taken => panic!("a root read is consumed once"),
                State::Abandoned => panic!("root reader panicked"),
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/source_reads.rs"]
mod tests;
