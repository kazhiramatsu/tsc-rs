//! An in-memory one-way pipe: what a peer served in the same process (a
//! test, or a content mapper run in-process) reads and writes in place of a
//! process's standard streams.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

#[derive(Default)]
struct Shared {
    /// The bytes written and not read yet, and whether the writer is gone.
    state: Mutex<(VecDeque<u8>, bool)>,
    ready: Condvar,
}

/// The reading end: blocks until bytes arrive, and ends once the writing
/// end is dropped.
pub struct PipeReader(Arc<Shared>);

/// The writing end; dropping it ends the reader's input.
pub struct PipeWriter(Arc<Shared>);

/// A new pipe.
pub fn pipe() -> (PipeReader, PipeWriter) {
    let shared = Arc::new(Shared::default());
    (PipeReader(Arc::clone(&shared)), PipeWriter(shared))
}

impl Read for PipeReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        while state.0.is_empty() && !state.1 {
            state = self
                .0
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let count = buffer.len().min(state.0.len());
        for (slot, byte) in buffer.iter_mut().zip(state.0.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .extend(bytes);
        self.0.ready.notify_all();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1 = true;
        self.0.ready.notify_all();
    }
}
