//! Owned bind batches activated below the loader's read/parse priority.
//!
//! The crew binds beside the thread that loads the program, and that thread's
//! chain (the ordered walk, the graph's finish, the publication) is what the
//! load waits for: the crew's threads start with the yielding threads' hook
//! ([`crate::workers::set_yielding_thread_start_hook`]).
use crate::{prepared::PendingSourceBind, WorkerBudget};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use tsc_types::IdentityDomain;

pub(crate) const MIN_ROOTS: usize = 256;

#[derive(Debug, Default)]
struct Pending {
    batches: Vec<Vec<PendingSourceBind>>,
    count: usize,
}

#[derive(Debug)]
struct Queue {
    jobs: Vec<PendingSourceBind>,
    started: usize,
}

/// Admission collects slots on parse workers. Activation only starts their
/// owner; that thread prepares the queue and participates in binding.
#[derive(Debug)]
pub(crate) struct SourceBinds {
    pending: Mutex<Option<Pending>>,
    domain: IdentityDomain,
    workers: WorkerBudget,
    tracing: bool,
    stopped: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SourceBinds {
    pub(crate) fn pending(
        domain: IdentityDomain,
        workers: WorkerBudget,
        tracing: bool,
    ) -> Arc<Self> {
        debug_assert!(workers.is_parallel());
        Arc::new(Self {
            pending: Mutex::new(Some(Pending::default())),
            domain,
            workers,
            tracing,
            stopped: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        })
    }

    pub(crate) fn register(&self, jobs: Vec<PendingSourceBind>) {
        if jobs.is_empty() {
            return;
        }
        let mut pending = self.pending.lock().expect("pending source binds");
        let pending = pending.as_mut().expect("registration precedes activation");
        pending.count += jobs.len();
        pending.batches.push(jobs);
    }

    pub(crate) fn activate(&self) -> bool {
        let domain = self.domain.clone();
        let tracing = self.tracing;
        self.activate_with(move |source| source.bind_reserved(&domain, tracing))
    }

    fn activate_with(&self, work: impl Fn(PendingSourceBind) + Send + Sync + 'static) -> bool {
        let mut handle = self.thread.lock().expect("source bind owner");
        if handle.is_some() {
            return true;
        }
        let pending = self.pending.lock().expect("pending source binds").take();
        let Some(pending) = pending else {
            return false;
        };
        if pending.count == 0 || self.stopped.load(Ordering::Acquire) {
            return false;
        }
        let count = (self.workers.max_workers() - 1).min(pending.count);
        let stopped = Arc::clone(&self.stopped);
        let owner = std::thread::Builder::new()
            .name("tsc-rs-bind".into())
            .stack_size(crate::workers::WORKER_STACK_BYTES)
            .spawn(move || {
                crate::workers::run_yielding_thread_start_hook();
                let eligible = pending.count;
                let mut jobs = Vec::with_capacity(eligible);
                let mut sources = pending.batches.into_iter().flatten();
                for source in sources.by_ref() {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    jobs.push(source);
                }
                drop(sources);
                jobs.reverse();
                let queue = Mutex::new(Queue { jobs, started: 0 });
                let run = || loop {
                    let job = {
                        let mut queue = queue.lock().expect("source bind queue");
                        if stopped.load(Ordering::Acquire) {
                            None
                        } else {
                            let job = queue.jobs.pop();
                            queue.started += usize::from(job.is_some());
                            job
                        }
                    };
                    let Some(job) = job else { break };
                    work(job);
                };
                std::thread::scope(|scope| {
                    for _ in 1..count {
                        let _ = std::thread::Builder::new()
                            .name("tsc-rs-bind".into())
                            .stack_size(crate::workers::WORKER_STACK_BYTES)
                            .spawn_scoped(scope, || {
                                crate::workers::run_yielding_thread_start_hook();
                                run();
                            });
                    }
                    run();
                });
                if tsc_types::trace::enabled() {
                    let queue = queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    eprintln!(
                        "[bind-tasks] started={} unstarted={}",
                        queue.started,
                        eligible - queue.started
                    );
                }
                // Queue, unused slots and callback captures all leave before
                // the owner joins and the consumer leases another identity.
            });
        match owner {
            Ok(owner) => {
                *handle = Some(owner);
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    pub(crate) fn finish(&self) {
        self.stop();
        // Keep the owner lock through join: a second concurrent consumer may
        // not pass the checkpoint while the first still reclaims root owners.
        let mut handle = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let failure = handle.take().and_then(|thread| thread.join().err());
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(handle);
        if let Some(panic) = failure {
            std::panic::resume_unwind(panic);
        }
    }

    #[cfg(test)]
    fn start_with(
        jobs: Vec<crate::PreparsedSyntax>,
        workers: WorkerBudget,
        work: impl Fn(PendingSourceBind) + Send + Sync + 'static,
    ) -> Option<Arc<Self>> {
        if jobs.is_empty() || !workers.is_parallel() {
            return None;
        }
        let crew = Self::pending(IdentityDomain::new(), workers, false);
        crew.register(
            jobs.iter()
                .map(crate::PreparsedSyntax::pending_bind)
                .collect(),
        );
        crew.activate_with(work).then_some(crew)
    }
}

impl Drop for SourceBinds {
    fn drop(&mut self) {
        self.stop();
        let handle = self
            .thread
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let failure = handle.take().and_then(|thread| thread.join().err());
        if !std::thread::panicking() {
            if let Some(panic) = failure {
                std::panic::resume_unwind(panic);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PreparsedSourceFile, PreparsedSyntax};
    use std::{
        num::NonZeroUsize,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use tsc_syntax::ParseOptions;

    #[test]
    fn stopping_yields_unstarted_slots_without_waiting_for_an_active_bind() {
        let domain = IdentityDomain::new();
        let slots: Vec<_> = (0..3)
            .map(|index| {
                let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                    format!("/{index}.ts"),
                    tsc_diagnostics::TextSnapshot::new(
                        format!("const a{index} = {index};"),
                        Default::default(),
                    ),
                    ParseOptions::default(),
                    None,
                    &domain,
                )
                .unwrap();
                let reservation = tsc_binder::BindReservations::lease(&source, &domain).unwrap();
                let now = Instant::now();
                let slot = PreparsedSyntax::new(
                    Box::new(PreparsedSourceFile::new(ParseOptions::default(), source)),
                    (now, now),
                );
                slot.reserve_bind(reservation);
                slot
            })
            .collect();
        let (started, active) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let released = Mutex::new(released);
        let crew = SourceBinds::start_with(
            slots.clone(),
            WorkerBudget::new(NonZeroUsize::new(2).unwrap()),
            move |slot| {
                started.send(()).unwrap();
                released.lock().unwrap().recv().unwrap();
                slot.bind_reserved(&domain, false);
            },
        )
        .unwrap();
        active.recv_timeout(Duration::from_secs(5)).unwrap();
        crew.stop();
        // The active worker still waits for this message: stop did not join it.
        let was_bound = |parsed: PreparsedSourceFile| {
            let (reservation, data, source) = parsed.into_bind_parts();
            let bound = data.is_some();
            drop(data);
            drop(reservation);
            drop(source);
            bound
        };
        assert!(!was_bound(slots[1].take().unwrap()));
        release.send(()).unwrap();
        drop(crew);
        assert!(was_bound(slots[0].take().unwrap()));
        assert!(!was_bound(slots[2].take().unwrap()));
    }
}

#[cfg(test)]
mod identity_handoff_tests {
    use super::*;
    use crate::PreparsedSyntax;
    use std::{num::NonZeroUsize, sync::mpsc, time::Duration};
    use tsc_types::IdentitySpace;

    #[test]
    fn the_handoff_releases_a_workers_last_identity_owner_before_new_leasing() {
        let domain = IdentityDomain::with_limits(Default::default()).unwrap();
        let held = domain.lease(IdentitySpace::Symbol, 1).unwrap();
        let original = held.range();
        let (started, active) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let released = Mutex::new(released);
        let crew = SourceBinds::start_with(
            vec![PreparsedSyntax::empty()],
            WorkerBudget::new(NonZeroUsize::new(2).unwrap()),
            move |_| {
                let _held = &held;
                started.send(()).unwrap();
                released.lock().unwrap().recv().unwrap();
            },
        )
        .unwrap();
        active.recv_timeout(Duration::from_secs(5)).unwrap();
        crew.stop();
        let premature = domain.lease(IdentitySpace::Symbol, 1).unwrap();
        assert_ne!(
            premature.range(),
            original,
            "the active worker still owns the earlier range"
        );
        release.send(()).unwrap();
        crew.finish();
        let next = domain.lease(IdentitySpace::Symbol, 1).unwrap();
        assert_eq!(
            next.range(),
            original,
            "handoff completed before the next lease"
        );
    }
}

#[cfg(test)]
mod yielding_start_tests {
    use super::*;
    use crate::PreparsedSyntax;
    use std::{num::NonZeroUsize, sync::atomic::AtomicUsize};

    static YIELDING_STARTS: AtomicUsize = AtomicUsize::new(0);

    fn count_start() {
        YIELDING_STARTS.fetch_add(1, Ordering::Relaxed);
    }

    /// The crew's owner and each of its scoped threads start with the
    /// yielding threads' hook: what the embedding asks of the threads that
    /// work beside the loading thread reaches all of them. (The hook is the
    /// process's: the crews of the other tests count too, so at least.)
    #[test]
    fn the_crews_threads_start_with_the_yielding_hook() {
        crate::workers::set_yielding_thread_start_hook(count_start);
        let before = YIELDING_STARTS.load(Ordering::Relaxed);
        let crew = SourceBinds::start_with(
            vec![PreparsedSyntax::empty(), PreparsedSyntax::empty()],
            WorkerBudget::new(NonZeroUsize::new(3).unwrap()),
            |_| {},
        )
        .unwrap();
        crew.finish();
        assert!(YIELDING_STARTS.load(Ordering::Relaxed) >= before + 2);
    }
}
