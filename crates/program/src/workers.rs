//! Explicit worker budget for the standard-library scoped concurrency used by
//! program loading (root parse-ahead) and checker construction (per-file
//! binding).
//!
//! tsrs-native: tsc has no equivalent; tsgo's `--singleThreaded` and checker
//! count are the closest controls. The budget is explicit so embedded callers
//! and tests choose their concurrency deterministically instead of inheriting
//! the machine's CPU count: the API default is [`WorkerBudget::serial`],
//! which is exactly the pre-concurrency behaviour, and the CLI selects
//! [`WorkerBudget::automatic`].

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// The most worker threads one budget ever uses, regardless of the machine.
///
/// This is a provisional resource cap, not a measured optimum: it bounds
/// the transient per-worker state (one parsed file each) and the thread
/// count an embedding sees, and it keeps the scoped steps well inside the
/// two-worker test/CI limits the project runs under. The scaling of the
/// parse-ahead and bind steps against worker count is measured separately
/// with explicit budgets; revisit the cap from that evidence.
pub const MAX_WORKERS: usize = 16;

/// Stack reserved for every thread the compiler starts: the workers, the
/// checker shards and the thread the `tsc-rs` command runs its work on.
///
/// The parser, binder, checker and emitter recurse on the native stack in
/// proportion to the nesting depth of the source, without a depth bound of
/// their own: a source is as deep as the stack allows. tsgo runs on Go
/// stacks that grow to 1 GB, so a source that tsgo compiles fits this
/// reservation too (the emit of the deepest shapes measured, nested object
/// literals and method chains, uses about 16 KiB of stack per level; the
/// 4,950-operator `binderBinaryExpressionStress` case needs about 80 MiB).
/// The reservation is virtual: pages are committed only as the recursion
/// touches them, so an ordinary source costs a thread no more than before.
/// A caller that runs the compiler on a thread of its own (the Rust API's
/// serial default runs on the calling thread) chooses that thread's stack;
/// the README's API section says so.
pub const WORKER_STACK_BYTES: usize = 1 << 30;

/// The hook every worker thread and checker shard runs first, installed at
/// most once per process by the embedding (the CLI asks the OS for
/// interactive scheduling, which on macOS steers the thread to the
/// performance cores). Libraries never install one; without a hook the
/// threads start as they always did.
static THREAD_START_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Install the thread-start hook; a second installation is ignored.
pub fn set_thread_start_hook(hook: fn()) {
    let _ = THREAD_START_HOOK.set(hook);
}

/// Run the installed thread-start hook, if any, on the calling thread.
pub fn run_thread_start_hook() {
    if let Some(hook) = THREAD_START_HOOK.get() {
        hook();
    }
}

/// The number of threads (including the calling thread) one scoped
/// parallel step may use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerBudget {
    max_workers: NonZeroUsize,
}

impl Default for WorkerBudget {
    /// The API default: no concurrency.
    fn default() -> Self {
        Self::serial()
    }
}

impl WorkerBudget {
    /// Exactly one worker: every step runs on the calling thread, which is
    /// the sequential behaviour and the reproducible serial control.
    pub const fn serial() -> Self {
        Self {
            max_workers: NonZeroUsize::MIN,
        }
    }

    /// An explicit worker count, clamped to [`MAX_WORKERS`].
    pub const fn new(max_workers: NonZeroUsize) -> Self {
        let clamped = if max_workers.get() > MAX_WORKERS {
            MAX_WORKERS
        } else {
            max_workers.get()
        };
        Self {
            // MAX_WORKERS is nonzero and the input was nonzero.
            max_workers: match NonZeroUsize::new(clamped) {
                Some(value) => value,
                None => NonZeroUsize::MIN,
            },
        }
    }

    /// `min(available_parallelism, MAX_WORKERS)`, at least one; the CLI
    /// default. `available_parallelism` failures fall back to serial.
    pub fn automatic() -> Self {
        let available = std::thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .clamp(1, MAX_WORKERS);
        Self::new(NonZeroUsize::new(available).unwrap_or(NonZeroUsize::MIN))
    }

    pub const fn max_workers(self) -> usize {
        self.max_workers.get()
    }

    /// Whether more than one worker may be used.
    pub const fn is_parallel(self) -> bool {
        self.max_workers.get() > 1
    }

    /// Run `work` over every job and return the results in job order.
    ///
    /// Jobs are handed out heaviest-first (by `weight`) from one shared
    /// counter. With a serial budget, or fewer than two jobs, everything runs
    /// on the calling thread. Otherwise the calling thread participates and
    /// up to `max_workers - 1` extra scoped threads, each with
    /// [`WORKER_STACK_BYTES`] of stack, join it; if the OS refuses a thread,
    /// the remaining threads simply share the jobs. A panic in any job
    /// propagates to the caller after every thread has finished; the panic
    /// hook's message then names the worker thread (`tsc-rs-worker`) rather
    /// than the calling thread, which is the one observable difference from
    /// a sequential panic.
    pub fn map_ordered<J, T>(
        self,
        jobs: Vec<J>,
        weight: impl Fn(&J) -> usize,
        work: impl Fn(J) -> T + Sync,
    ) -> Vec<T>
    where
        J: Send,
        T: Send,
    {
        let workers = self.max_workers().min(jobs.len());
        if workers < 2 {
            return jobs.into_iter().map(work).collect();
        }
        let mut order: Vec<usize> = (0..jobs.len()).collect();
        order.sort_by_key(|&index| std::cmp::Reverse(weight(&jobs[index])));
        let slots: Vec<Mutex<(Option<J>, Option<T>)>> = jobs
            .into_iter()
            .map(|job| Mutex::new((Some(job), None)))
            .collect();
        let next = AtomicUsize::new(0);
        let run = || loop {
            let Some(&index) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                break;
            };
            let job = slots[index]
                .lock()
                .expect("worker slot")
                .0
                .take()
                .expect("each job is taken once");
            let result = work(job);
            slots[index].lock().expect("worker slot").1 = Some(result);
        };
        std::thread::scope(|scope| {
            for _ in 1..workers {
                // A refused thread is not an error: the calling thread and
                // the threads already started finish the jobs.
                let _ = std::thread::Builder::new()
                    .name("tsc-rs-worker".to_owned())
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, || {
                        run_thread_start_hook();
                        run();
                    });
            }
            run();
        });
        slots
            .into_iter()
            .map(|slot| {
                slot.into_inner()
                    .expect("worker slot")
                    .1
                    .expect("every job produced a result")
            })
            .collect()
    }

    /// Run `work` over jobs that `produce` yields one at a time on the
    /// calling thread, and return the results in production order.
    ///
    /// `produce(ordinal)` is called for ordinals `0..capacity` in order,
    /// on the calling thread only, and stops at the first `None`; every job
    /// it yields is handed to a worker as soon as it exists, so production
    /// (host reads) overlaps the work (parsing) already under way. With a
    /// serial budget everything runs on the calling thread in order. The
    /// thread and panic protocol is that of [`Self::map_ordered`]; a panic
    /// in `produce` still releases the waiting workers before it propagates.
    pub fn map_streamed<J, T>(
        self,
        capacity: usize,
        mut produce: impl FnMut(usize) -> Option<J>,
        work: impl Fn(J) -> T + Sync,
    ) -> Vec<T>
    where
        J: Send,
        T: Send,
    {
        let workers = self.max_workers().min(capacity);
        if workers < 2 {
            let mut results = Vec::with_capacity(capacity);
            while results.len() < capacity {
                let Some(job) = produce(results.len()) else {
                    break;
                };
                results.push(work(job));
            }
            return results;
        }
        struct Stream<J> {
            pending: VecDeque<(usize, J)>,
            closed: bool,
        }
        struct Shared<J, T> {
            stream: Mutex<Stream<J>>,
            ready: Condvar,
            results: Vec<Mutex<Option<T>>>,
        }
        let shared = Shared {
            stream: Mutex::new(Stream {
                pending: VecDeque::new(),
                closed: false,
            }),
            ready: Condvar::new(),
            results: (0..capacity).map(|_| Mutex::new(None)).collect(),
        };
        let run = || loop {
            let job = {
                let mut stream = shared.stream.lock().expect("read-ahead stream");
                loop {
                    if let Some(job) = stream.pending.pop_front() {
                        break Some(job);
                    }
                    if stream.closed {
                        break None;
                    }
                    stream = shared.ready.wait(stream).expect("read-ahead stream");
                }
            };
            let Some((ordinal, job)) = job else {
                break;
            };
            let result = work(job);
            *shared.results[ordinal].lock().expect("read-ahead result") = Some(result);
        };
        // Closing the stream on every exit (including an unwinding
        // producer) releases the workers so the scope can join them.
        struct CloseOnDrop<'shared, J, T>(&'shared Shared<J, T>);
        impl<J, T> Drop for CloseOnDrop<'_, J, T> {
            fn drop(&mut self) {
                if let Ok(mut stream) = self.0.stream.lock() {
                    stream.closed = true;
                }
                self.0.ready.notify_all();
            }
        }
        let produced = std::thread::scope(|scope| {
            for _ in 1..workers {
                // A refused thread is not an error: the calling thread and
                // the threads already started finish the jobs.
                let _ = std::thread::Builder::new()
                    .name("tsc-rs-worker".to_owned())
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, || {
                        run_thread_start_hook();
                        run();
                    });
            }
            let mut produced = 0;
            {
                let _close = CloseOnDrop(&shared);
                while produced < capacity {
                    let Some(job) = produce(produced) else {
                        break;
                    };
                    shared
                        .stream
                        .lock()
                        .expect("read-ahead stream")
                        .pending
                        .push_back((produced, job));
                    shared.ready.notify_one();
                    produced += 1;
                }
            }
            run();
            produced
        });
        shared
            .results
            .into_iter()
            .take(produced)
            .map(|slot| {
                slot.into_inner()
                    .expect("read-ahead result")
                    .expect("every produced job has a result")
            })
            .collect()
    }

    /// Run `work` over `seeds` and over every job the work spawns, the way
    /// tsgo's work group queues a goroutine per parse task and Go's
    /// scheduler steals them between threads.
    ///
    /// Each worker owns a deque. The seeds are dealt round-robin, so each
    /// deque holds every `workers`-th seed with the first seed at its back;
    /// a worker takes from the back of its own deque (its first seed first,
    /// then the newest job it spawned), and a worker whose deque is empty
    /// takes from the front of another's. `state` builds each worker's state
    /// once, so it can keep caches warm across that worker's jobs; `work`
    /// gets it, the job and a vector to push the jobs it spawns into, and
    /// `finish` turns it into what the worker hands back. Both run on the
    /// worker's own thread, so the state need not be `Send`. The result is
    /// every worker's `finish` value with its results, in no particular
    /// order. With a serial budget everything runs on the calling thread. A
    /// panic in a job releases the other workers, then propagates as
    /// [`Self::map_ordered`]'s does.
    pub fn map_spawning<J, S, T, U>(
        self,
        seeds: Vec<J>,
        state: impl Fn() -> S + Sync,
        work: impl Fn(&mut S, J, &mut Vec<J>) -> Option<T> + Sync,
        finish: impl Fn(S) -> U + Sync,
    ) -> Vec<(U, Vec<T>)>
    where
        J: Send,
        T: Send,
        U: Send,
    {
        if seeds.is_empty() {
            return Vec::new();
        }
        let workers = self.max_workers();
        if workers < 2 {
            let mut worker_state = state();
            let mut results = Vec::new();
            let mut spawned = Vec::new();
            let mut stack: Vec<J> = seeds.into_iter().rev().collect();
            while let Some(job) = stack.pop() {
                results.extend(work(&mut worker_state, job, &mut spawned));
                stack.append(&mut spawned);
            }
            return vec![(finish(worker_state), results)];
        }
        let mut dealt: Vec<VecDeque<J>> = (0..workers).map(|_| VecDeque::new()).collect();
        let seed_count = seeds.len();
        for (index, seed) in seeds.into_iter().enumerate() {
            dealt[index % workers].push_front(seed);
        }
        let pool = SpawningPool {
            deques: dealt.into_iter().map(Mutex::new).collect(),
            pending: AtomicUsize::new(seed_count),
            queued: AtomicUsize::new(seed_count),
            sleeping: AtomicUsize::new(0),
            aborted: AtomicBool::new(false),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
        };
        let run = |worker: usize| {
            let _release = ReleaseOnUnwind(&pool);
            let mut worker_state = state();
            let mut results = Vec::new();
            let mut spawned = Vec::new();
            loop {
                if pool.aborted.load(Ordering::SeqCst) {
                    break;
                }
                let Some(job) = pool.take(worker) else {
                    if pool.pending.load(Ordering::SeqCst) == 0 {
                        break;
                    }
                    pool.wait();
                    continue;
                };
                results.extend(work(&mut worker_state, job, &mut spawned));
                if !spawned.is_empty() {
                    pool.push(worker, &mut spawned);
                }
                if pool.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                    pool.wake_all();
                }
            }
            (finish(worker_state), results)
        };
        std::thread::scope(|scope| {
            let handles = (1..workers)
                .map(|worker| {
                    let run = &run;
                    std::thread::Builder::new()
                        .name("tsc-rs-worker".to_owned())
                        .stack_size(WORKER_STACK_BYTES)
                        .spawn_scoped(scope, move || {
                            run_thread_start_hook();
                            run(worker)
                        })
                })
                .collect::<Vec<_>>();
            // A refused thread is not an error: its deque is stolen from.
            let mut finished = vec![run(0)];
            for handle in handles.into_iter().flatten() {
                match handle.join() {
                    Ok(worker) => finished.push(worker),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
            finished
        })
    }
}

/// The shared state of [`WorkerBudget::map_spawning`].
struct SpawningPool<J> {
    deques: Vec<Mutex<VecDeque<J>>>,
    /// Jobs queued or running: zero once the work is done.
    pending: AtomicUsize,
    /// Jobs in the deques.
    queued: AtomicUsize,
    /// Workers waiting for a job.
    sleeping: AtomicUsize,
    aborted: AtomicBool,
    sleep: Mutex<()>,
    wake: Condvar,
}

impl<J> SpawningPool<J> {
    /// The newest job of `worker`'s deque, else the oldest of another's.
    fn take(&self, worker: usize) -> Option<J> {
        if self.queued.load(Ordering::SeqCst) == 0 {
            return None;
        }
        // One deque locked at a time: a worker that held its own while
        // stealing would deadlock with one stealing from it.
        let own = self.deques[worker].lock().expect("worker deque").pop_back();
        let count = self.deques.len();
        let job = own.or_else(|| {
            (1..count).find_map(|offset| {
                let mut deque = self.deques[(worker + offset) % count]
                    .lock()
                    .expect("worker deque");
                deque.pop_front()
            })
        })?;
        self.queued.fetch_sub(1, Ordering::SeqCst);
        Some(job)
    }

    /// Queue the jobs a job of `worker` spawned, waking waiting workers.
    fn push(&self, worker: usize, spawned: &mut Vec<J>) {
        let count = spawned.len();
        self.pending.fetch_add(count, Ordering::SeqCst);
        // Counted before they are visible, so a taker never counts down a
        // job that is not counted yet.
        self.queued.fetch_add(count, Ordering::SeqCst);
        self.deques[worker]
            .lock()
            .expect("worker deque")
            .extend(spawned.drain(..));
        if self.sleeping.load(Ordering::SeqCst) > 0 {
            self.wake_all();
        }
    }

    /// Wait for a queued job, the end of the work or an abort. A worker
    /// counts itself as sleeping before it checks for work, and a push
    /// queues before it checks for sleepers, so a wake-up is never lost;
    /// the timeout only bounds an unexpected one.
    fn wait(&self) {
        let guard = self.sleep.lock().expect("worker sleep");
        self.sleeping.fetch_add(1, Ordering::SeqCst);
        if self.queued.load(Ordering::SeqCst) == 0
            && self.pending.load(Ordering::SeqCst) != 0
            && !self.aborted.load(Ordering::SeqCst)
        {
            let _ = self
                .wake
                .wait_timeout(guard, std::time::Duration::from_millis(1))
                .expect("worker sleep");
        }
        self.sleeping.fetch_sub(1, Ordering::SeqCst);
    }

    fn wake_all(&self) {
        let _guard = self.sleep.lock().expect("worker sleep");
        self.wake.notify_all();
    }
}

/// Releases the other workers when a job panics, so the scope can join them
/// and propagate the panic.
struct ReleaseOnUnwind<'pool, J>(&'pool SpawningPool<J>);

impl<J> Drop for ReleaseOnUnwind<'_, J> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.aborted.store(true, Ordering::SeqCst);
            if let Ok(_guard) = self.0.sleep.lock() {
                self.0.wake.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_are_explicit_and_bounded() {
        assert_eq!(WorkerBudget::default(), WorkerBudget::serial());
        assert_eq!(WorkerBudget::serial().max_workers(), 1);
        assert!(!WorkerBudget::serial().is_parallel());
        assert_eq!(
            WorkerBudget::new(NonZeroUsize::new(3).unwrap()).max_workers(),
            3
        );
        assert_eq!(
            WorkerBudget::new(NonZeroUsize::new(1_000).unwrap()).max_workers(),
            MAX_WORKERS
        );
        let automatic = WorkerBudget::automatic().max_workers();
        assert!((1..=MAX_WORKERS).contains(&automatic));
    }

    #[test]
    fn map_ordered_returns_results_in_job_order_for_every_budget() {
        let jobs: Vec<u64> = (0..37).collect();
        let expected: Vec<u64> = jobs.iter().map(|job| job * job).collect();
        for workers in [1, 2, 4, MAX_WORKERS] {
            let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
            let actual = budget.map_ordered(jobs.clone(), |&job| job as usize, |job| job * job);
            assert_eq!(actual, expected, "workers={workers}");
        }
        let single = WorkerBudget::new(NonZeroUsize::new(4).unwrap()).map_ordered(
            vec![7u64],
            |_| 0,
            |job| job + 1,
        );
        assert_eq!(single, [8]);
        let empty: Vec<u64> =
            WorkerBudget::automatic().map_ordered(Vec::<u64>::new(), |_| 0, |job| job);
        assert!(empty.is_empty());
    }

    /// Every job runs exactly once, results keep job order, and the observed
    /// peak concurrency never exceeds the (clamped) budget. Adapted from the
    /// integrator's independent probe of this module
    /// (review/worker-budget-probe-20260922).
    #[test]
    fn each_job_runs_once_and_peak_concurrency_respects_the_budget() {
        use std::time::Duration;
        for requested in [1usize, 2, 4, 8, 64] {
            let budget = WorkerBudget::new(NonZeroUsize::new(requested).unwrap());
            let seen: Vec<_> = (0..64).map(|_| AtomicUsize::new(0)).collect();
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            let result = budget.map_ordered(
                (0..64usize).collect(),
                |&job| 64 - job,
                |job| {
                    seen[job].fetch_add(1, Ordering::SeqCst);
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(1));
                    active.fetch_sub(1, Ordering::SeqCst);
                    (job, job * 3 + 7)
                },
            );
            assert_eq!(
                result,
                (0..64).map(|job| (job, job * 3 + 7)).collect::<Vec<_>>()
            );
            assert!(seen
                .iter()
                .all(|counter| counter.load(Ordering::SeqCst) == 1));
            assert_eq!(active.load(Ordering::SeqCst), 0);
            let actual_peak = peak.load(Ordering::SeqCst);
            assert!(
                (1..=budget.max_workers()).contains(&actual_peak),
                "requested={requested} allowed={} observed_peak={actual_peak}",
                budget.max_workers()
            );
        }
    }

    /// Every seed and every spawned job runs exactly once, whatever the
    /// budget, and each worker's state sees only its own jobs.
    #[test]
    fn map_spawning_runs_every_seed_and_spawned_job_once() {
        // Job n spawns 2n+1 and 2n+2 below 4,000: a binary tree over 0..4000
        // seeded with its first 3 nodes' children's parents.
        let limit = 4_000usize;
        for workers in [1, 2, 4, MAX_WORKERS] {
            let budget = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
            let seen: Vec<_> = (0..limit).map(|_| AtomicUsize::new(0)).collect();
            let finished = budget.map_spawning(
                vec![0usize],
                || 0usize,
                |jobs_here, job, spawned| {
                    seen[job].fetch_add(1, Ordering::SeqCst);
                    *jobs_here += 1;
                    for child in [2 * job + 1, 2 * job + 2] {
                        if child < limit {
                            spawned.push(child);
                        }
                    }
                    (job % 3 == 0).then_some(job)
                },
                |jobs_here| jobs_here,
            );
            assert!(seen.iter().all(|count| count.load(Ordering::SeqCst) == 1));
            assert_eq!(
                finished.iter().map(|(jobs, _)| jobs).sum::<usize>(),
                limit,
                "workers={workers}"
            );
            let mut results: Vec<usize> = finished
                .into_iter()
                .flat_map(|(_, results)| results)
                .collect();
            results.sort_unstable();
            assert_eq!(
                results,
                (0..limit).filter(|job| job % 3 == 0).collect::<Vec<_>>()
            );
        }
        assert!(WorkerBudget::automatic()
            .map_spawning(Vec::<usize>::new(), || (), |_, job, _| Some(job), |()| ())
            .is_empty());
    }

    #[test]
    fn map_spawning_propagates_a_job_panic() {
        let outcome = std::panic::catch_unwind(|| {
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()).map_spawning(
                (0..64usize).collect(),
                || (),
                |_, job, spawned| {
                    assert_ne!(job, 70, "job seventy fails");
                    if job < 32 {
                        spawned.push(job + 64);
                    }
                    Some(job)
                },
                |()| (),
            )
        });
        assert!(outcome.is_err());
    }

    #[test]
    fn map_ordered_propagates_a_job_panic() {
        let outcome = std::panic::catch_unwind(|| {
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()).map_ordered(
                vec![0u8, 1, 2, 3, 4, 5],
                |_| 0,
                |job| {
                    assert_ne!(job, 3, "job three fails");
                    job
                },
            )
        });
        assert!(outcome.is_err());
    }
}
