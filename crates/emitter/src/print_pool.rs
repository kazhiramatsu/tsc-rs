//! Scoped execution of owned emit work on the unused part of a worker budget.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Condvar, Mutex};

type Panic = Box<dyn Any + Send + 'static>;
type Job = Box<dyn FnOnce(&mut Option<crate::Printer>) + Send + 'static>;

#[derive(Clone, Copy, Debug, Default)]
pub struct PrintPoolPeak {
    pub units: usize,
    pub charged_bytes: usize,
    pub submitted: usize,
    pub workers: usize,
}

#[derive(Default)]
struct Usage {
    units: usize,
    charged_bytes: usize,
    peak: PrintPoolPeak,
}

#[derive(Default)]
struct QueueState {
    jobs: VecDeque<Job>,
    closed: bool,
    retired: u64,
}

#[derive(Default)]
struct WorkQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

struct Credits {
    queue: Arc<WorkQueue>,
    usage: Mutex<Usage>,
    available: Condvar,
    units: usize,
    bytes: usize,
}

/// Acquired before a producer retains its next input, and retained through
/// printing, cleanup and delivery of the result to the owner.
#[derive(Clone)]
pub(crate) struct PrintCredit(Arc<CreditState>);

struct CreditState {
    credits: Arc<Credits>,
    charged_bytes: std::sync::atomic::AtomicUsize,
}

impl PrintCredit {
    /// A failed byte reservation leaves the unit on its owner for inline
    /// completion. No producer waits with an uncharged queued payload.
    pub(crate) fn try_charge(&mut self, bytes: usize) -> bool {
        let mut usage = self.0.credits.usage.lock().expect("emit credits");
        let Some(total) = usage.charged_bytes.checked_add(bytes) else {
            return false;
        };
        if total > self.0.credits.bytes {
            return false;
        }
        usage.charged_bytes = total;
        self.0
            .charged_bytes
            .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
        usage.peak.charged_bytes = usage.peak.charged_bytes.max(total);
        true
    }
}

impl Drop for CreditState {
    fn drop(&mut self) {
        let mut usage = self.credits.usage.lock().expect("emit credits");
        usage.units -= 1;
        usage.charged_bytes -= self
            .charged_bytes
            .load(std::sync::atomic::Ordering::Relaxed);
        self.credits.available.notify_all();
        drop(usage);
        // Results were allocated by a print worker and can be freed here
        // by the owner. Wake idle workers so their allocator can collect
        // those remote frees before more output accumulates.
        let mut queue = self.credits.queue.state.lock().expect("emit queue");
        queue.retired = queue.retired.wrapping_add(1);
        self.credits.queue.ready.notify_all();
    }
}

pub(crate) struct PendingPrint<T> {
    result: mpsc::Receiver<(Result<T, Panic>, PrintCredit)>,
    ready: Option<(Result<T, Panic>, PrintCredit)>,
}

impl<T> PendingPrint<T> {
    pub(crate) fn is_ready(&mut self) -> bool {
        if self.ready.is_none() {
            match self.result.try_recv() {
                Ok(result) => self.ready = Some(result),
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => panic!("emit worker returned no result"),
            }
        }
        true
    }

    pub(crate) fn finish(mut self) -> (T, PrintCredit) {
        let (result, credit) = self
            .ready
            .take()
            .unwrap_or_else(|| self.result.recv().expect("emit worker returned a result"));
        match result {
            Ok(result) => (result, credit),
            Err(panic) => resume_unwind(panic),
        }
    }
}

#[derive(Default)]
struct Workers {
    handles: Vec<std::thread::JoinHandle<()>>,
    refused: bool,
}

pub struct EmitPrintPool {
    queue: Arc<WorkQueue>,
    credits: Arc<Credits>,
    workers: Mutex<Workers>,
    spare: usize,
    reserved: std::sync::atomic::AtomicUsize,
    active: Arc<std::sync::atomic::AtomicUsize>,
    owners: usize,
    start_failure: Arc<Mutex<Option<Panic>>>,
}

impl EmitPrintPool {
    /// Existing source preparation reserves its share until it completes.
    pub fn set_reserved_workers(&self, count: usize) {
        self.reserved
            .store(count, std::sync::atomic::Ordering::Release);
    }

    fn start_workers(&self) -> bool {
        use std::sync::atomic::Ordering;
        let mut workers = self.workers.lock().expect("emit workers");
        let limit = self
            .spare
            .saturating_sub(self.reserved.load(Ordering::Acquire))
            .min(self.owners * 2)
            .min(3)
            .min(self.active.load(Ordering::Acquire));
        while workers.handles.len() < limit && !workers.refused {
            let queue = Arc::clone(&self.queue);
            let failure = Arc::clone(&self.start_failure);
            match std::thread::Builder::new()
                .name("tsc-rs-worker".to_owned())
                .stack_size(tsc_program::WORKER_STACK_BYTES)
                .spawn(move || {
                    if let Err(panic) = catch_unwind(tsc_program::run_thread_start_hook) {
                        *failure.lock().expect("worker start failure") = Some(panic);
                    }
                    let mut printer = None;
                    let mut collected = None;
                    loop {
                        let mut state = queue.state.lock().expect("emit queue");
                        let job = loop {
                            if let Some(job) = state.jobs.pop_front() {
                                break Some(job);
                            }
                            if state.closed {
                                break None;
                            }
                            if collected != Some(state.retired) {
                                let retired = state.retired;
                                drop(state);
                                if let Err(panic) = catch_unwind(tsc_program::run_worker_idle_hook)
                                {
                                    failure
                                        .lock()
                                        .expect("worker idle failure")
                                        .get_or_insert(panic);
                                }
                                collected = Some(retired);
                                state = queue.state.lock().expect("emit queue");
                            } else {
                                state = queue.ready.wait(state).expect("emit queue");
                            }
                        };
                        drop(state);
                        match job {
                            Some(job) => {
                                job(&mut printer);
                                collected = None;
                            }
                            None => break,
                        }
                    }
                }) {
                Ok(handle) => workers.handles.push(handle),
                Err(_) => workers.refused = true,
            }
        }
        self.credits
            .usage
            .lock()
            .expect("emit credits")
            .peak
            .workers = workers.handles.len();
        !workers.handles.is_empty()
    }

    pub(crate) fn acquire(&self) -> PrintCredit {
        let mut usage = self.credits.usage.lock().expect("emit credits");
        while usage.units == self.credits.units {
            usage = self.credits.available.wait(usage).expect("emit credits");
        }
        usage.units += 1;
        usage.peak.units = usage.peak.units.max(usage.units);
        PrintCredit(Arc::new(CreditState {
            credits: Arc::clone(&self.credits),
            charged_bytes: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    pub(crate) fn submit<T: Send + 'static>(
        &self,
        credit: PrintCredit,
        work: impl FnOnce(&mut Option<crate::Printer>) -> T + Send + 'static,
    ) -> PendingPrint<T> {
        self.credits
            .usage
            .lock()
            .expect("emit credits")
            .peak
            .submitted += 1;
        let (sender, result) = mpsc::sync_channel(1);
        let active = Arc::clone(&self.active);
        active.fetch_add(1, std::sync::atomic::Ordering::Release);
        let job: Job = Box::new(move |printer: &mut Option<crate::Printer>| {
            let output = catch_unwind(AssertUnwindSafe(|| work(printer)));
            if output.is_err() {
                *printer = None;
            }
            active.fetch_sub(1, std::sync::atomic::Ordering::Release);
            // A canceled owner still releases the result and its credit here.
            let _ = sender.send((output, credit));
        });
        if self.start_workers() {
            let mut queue = self.queue.state.lock().expect("emit queue");
            assert!(!queue.closed, "emit pool is open");
            // The producer acquired a unit credit before retaining this input.
            queue.jobs.push_back(job);
            self.queue.ready.notify_one();
        } else {
            // A refused thread leaves this exact work on its owner.
            job(&mut None);
        }
        PendingPrint {
            result,
            ready: None,
        }
    }

    pub fn peak(&self) -> PrintPoolPeak {
        self.credits.usage.lock().expect("emit credits").peak
    }
}

impl Drop for EmitPrintPool {
    fn drop(&mut self) {
        {
            let mut queue = self.queue.state.lock().expect("emit queue");
            queue.closed = true;
            self.queue.ready.notify_all();
        }
        let handles = std::mem::take(&mut self.workers.lock().expect("emit workers").handles);
        let mut failure = None;
        for handle in handles {
            if let Err(panic) = handle.join() {
                failure.get_or_insert(panic);
            }
        }
        failure = failure.or_else(|| {
            self.start_failure
                .lock()
                .expect("worker start failure")
                .take()
        });
        if !std::thread::panicking() {
            if let Some(panic) = failure {
                resume_unwind(panic);
            }
        }
    }
}

impl EmitPrintPool {
    pub fn for_budget(workers: usize, owners: usize, byte_limit: usize) -> Option<Self> {
        let spare = workers.saturating_sub(owners);
        if spare == 0 || owners == 0 {
            return None;
        }
        let capacity = workers + owners;
        let queue = Arc::new(WorkQueue::default());
        let pool = Self {
            queue: Arc::clone(&queue),
            credits: Arc::new(Credits {
                queue,
                usage: Mutex::new(Usage::default()),
                available: Condvar::new(),
                units: capacity,
                bytes: byte_limit,
            }),
            workers: Mutex::new(Workers::default()),
            spare,
            reserved: std::sync::atomic::AtomicUsize::new(0),
            active: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            owners,
            start_failure: Arc::new(Mutex::new(None)),
        };
        Some(pool)
    }
}

#[cfg(test)]
fn with_print_pool<T>(
    workers: usize,
    owners: usize,
    byte_limit: usize,
    operation: impl FnOnce(Option<&EmitPrintPool>) -> T,
) -> T {
    let pool = EmitPrintPool::for_budget(workers, owners, byte_limit);
    operation(pool.as_ref())
}

#[cfg(test)]
#[path = "../tests/unit/print_pool.rs"]
mod tests;
