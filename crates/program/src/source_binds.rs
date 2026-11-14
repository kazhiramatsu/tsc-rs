//! Owned bind work below the loader's read/parse priority.
use crate::{PreparsedSyntax, WorkerBudget};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tsc_types::IdentityDomain;

// Short commands have too little ordered load work to hide a crew's startup.
pub(crate) const MIN_ROOTS: usize = 256;

#[derive(Debug)]
struct Queue {
    jobs: Vec<PreparsedSyntax>,
    stopped: bool,
    started: usize,
}

/// Workers own the queue, while the Program alone owns their join handles.
/// Queue entries hold existing slots, never another tree or completed bind.
#[derive(Debug)]
pub(crate) struct SourceBinds {
    queue: Arc<Mutex<Queue>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl SourceBinds {
    pub(crate) fn start(
        jobs: Vec<PreparsedSyntax>,
        domain: IdentityDomain,
        workers: WorkerBudget,
        tracing: bool,
    ) -> Option<Arc<Self>> {
        Self::start_with(jobs, workers, move |syntax| {
            syntax.bind_reserved(&domain, tracing);
        })
    }

    fn start_with(
        mut jobs: Vec<PreparsedSyntax>,
        workers: WorkerBudget,
        work: impl Fn(PreparsedSyntax) + Send + Sync + 'static,
    ) -> Option<Arc<Self>> {
        if jobs.is_empty() || !workers.is_parallel() {
            return None;
        }
        let count = (workers.max_workers() - 1).min(jobs.len());
        jobs.reverse();
        let queue = Arc::new(Mutex::new(Queue {
            jobs,
            stopped: false,
            started: 0,
        }));
        let work = Arc::new(work);
        let mut threads = Vec::with_capacity(count);
        for _ in 0..count {
            let queue = Arc::clone(&queue);
            let work = Arc::clone(&work);
            if let Ok(thread) = std::thread::Builder::new()
                .name("tsc-rs-bind".into())
                .stack_size(crate::workers::WORKER_STACK_BYTES)
                .spawn(move || {
                    crate::workers::run_thread_start_hook();
                    loop {
                        let job = {
                            let mut queue = queue.lock().expect("source bind queue");
                            if queue.stopped {
                                None
                            } else {
                                let job = queue.jobs.pop();
                                queue.started += usize::from(job.is_some());
                                job
                            }
                        };
                        let Some(job) = job else { break };
                        work(job);
                    }
                })
            {
                threads.push(thread);
            }
        }
        if threads.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            queue,
            threads: Mutex::new(threads),
        }))
    }

    /// Reclaim active work before the consumer leases another identity.
    /// In particular, a worker may be the last owner of an unvisited parse.
    pub(crate) fn finish(&self) {
        self.stop();
        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut failure = None;
        for thread in threads.drain(..) {
            if let Err(panic) = thread.join() {
                failure = Some(panic);
            }
        }
        drop(threads);
        if let Some(panic) = failure {
            std::panic::resume_unwind(panic);
        }
    }

    /// The checker adopts completed slots and binds the unstarted remainder.
    /// This waits only for the queue lock, never for an in-flight bind.
    pub(crate) fn stop(&self) {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !queue.stopped && tsc_types::trace::enabled() {
            eprintln!(
                "[bind-tasks] started={} unstarted={}",
                queue.started,
                queue.jobs.len()
            );
        }
        queue.stopped = true;
        queue.jobs.clear();
    }
}

impl Drop for SourceBinds {
    fn drop(&mut self) {
        self.stop();
        let threads = self
            .threads
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut failure = None;
        for thread in threads.drain(..) {
            if let Err(panic) = thread.join() {
                failure = Some(panic);
            }
        }
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
    use crate::PreparsedSourceFile;
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
