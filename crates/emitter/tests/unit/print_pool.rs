use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn an_unused_pool_starts_no_workers_and_serial_work_has_no_pool() {
    with_print_pool(1, 1, 64, |pool| assert!(pool.is_none()));
    with_print_pool(10, 4, 64, |pool| {
        let peak = pool.unwrap().peak();
        assert_eq!(
            (peak.workers, peak.submitted, peak.units, peak.charged_bytes),
            (0, 0, 0, 0)
        );
    });
}

#[test]
fn input_bytes_retire_before_the_unit_result_is_delivered() {
    with_print_pool(4, 1, 64, |pool| {
        let pool = pool.unwrap();
        let first = pool.acquire();
        let first_charge = first.try_charge(40).unwrap();
        let second = pool.acquire();
        assert!(second.try_charge(25).is_none());
        let second_charge = second.try_charge(24).unwrap();
        assert_eq!(pool.credits.usage.lock().unwrap().charged_bytes, 64);
        drop(first_charge);
        let usage = pool.credits.usage.lock().unwrap();
        assert_eq!((usage.units, usage.charged_bytes), (2, 24));
        drop(usage);
        let replacement = first.try_charge(40).unwrap();
        drop(first);
        assert_eq!(pool.credits.usage.lock().unwrap().units, 2);
        drop(replacement);
        assert_eq!(pool.credits.usage.lock().unwrap().units, 1);
        drop(second_charge);
        drop(second);
        let usage = pool.credits.usage.lock().unwrap();
        assert_eq!((usage.units, usage.charged_bytes), (0, 0));
    });
}

#[test]
fn owned_jobs_execute_on_spare_workers_and_deliver_in_owner_order() {
    let owner = std::thread::current().id();
    with_print_pool(4, 1, 64, |pool| {
        let pool = pool.unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut pending = Vec::new();
        for index in 0..3 {
            let barrier = Arc::clone(&barrier);
            let credit = pool.acquire();
            let charge = credit.try_charge(16).unwrap();
            pending.push(pool.submit(credit, move |_| {
                let _charge = charge;
                if index < 2 {
                    barrier.wait();
                }
                assert_ne!(std::thread::current().id(), owner);
                index
            }));
        }
        barrier.wait();
        let actual = pending
            .into_iter()
            .map(|work| {
                let (value, credit) = work.finish();
                drop(credit);
                value
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, [0, 1, 2]);
        let peak = pool.peak();
        assert_eq!(
            (peak.submitted, peak.workers, peak.units, peak.charged_bytes),
            (3, 2, 3, 48)
        );
    });
}

#[test]
fn a_worker_panic_reaches_its_owner_without_stranding_later_jobs() {
    with_print_pool(2, 1, 64, |pool| {
        let pool = pool.unwrap();
        let failure: PendingPrint<()> = pool.submit(pool.acquire(), |cached| {
            *cached = Some(crate::create_printer(crate::PrinterOptions::new(
                crate::NewLineKind::LineFeed,
            )));
            panic!("print fixture panic")
        });
        let success = pool.submit(pool.acquire(), |cached| {
            assert!(cached.is_none(), "a failed worker printer is discarded");
            7
        });
        assert!(catch_unwind(AssertUnwindSafe(|| failure.finish())).is_err());
        let (actual, credit) = success.finish();
        drop(credit);
        assert_eq!(actual, 7);
    });
}

#[test]
fn canceled_results_release_their_owned_work_before_shutdown_returns() {
    let credits = with_print_pool(4, 1, 64, |pool| {
        let pool = pool.unwrap();
        let pending = pool.submit(pool.acquire(), |_| 7);
        drop(pending);
        Arc::clone(&pool.credits)
    });
    let usage = credits.usage.lock().unwrap();
    assert_eq!((usage.units, usage.charged_bytes), (0, 0));
}

#[test]
fn source_preparers_reserve_workers_until_their_work_is_finished() {
    with_print_pool(4, 2, 64, |pool| {
        let pool = pool.unwrap();
        pool.set_reserved_workers(2);
        let owner = std::thread::current().id();
        let inline = pool.submit(pool.acquire(), move |_| {
            std::thread::current().id() == owner
        });
        let (inline, credit) = inline.finish();
        drop(credit);
        assert!(inline);
        assert_eq!(pool.peak().workers, 0);
        pool.set_reserved_workers(0);
        let worker = pool.submit(pool.acquire(), move |_| {
            std::thread::current().id() != owner
        });
        let (worker, credit) = worker.finish();
        drop(credit);
        assert!(worker);
        assert_eq!(pool.peak().workers, 1);
    });
}

thread_local! {
    static IDLE_OBSERVER: std::cell::RefCell<Option<(Arc<std::sync::atomic::AtomicBool>, mpsc::Sender<bool>)>> = const { std::cell::RefCell::new(None) };
}

fn observe_worker_idle() {
    IDLE_OBSERVER.with(|observer| {
        if let Some((retired, sender)) = observer.borrow().as_ref() {
            let _ = sender.send(retired.load(std::sync::atomic::Ordering::Acquire));
        }
    });
}

#[test]
fn retiring_a_result_wakes_its_idle_worker_without_another_job() {
    tsc_program::set_worker_idle_hook(observe_worker_idle);
    let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sender, idle) = mpsc::channel();
    with_print_pool(2, 1, 64, |pool| {
        let pool = pool.unwrap();
        let signal = Arc::clone(&retired);
        let pending = pool.submit(pool.acquire(), move |_| {
            IDLE_OBSERVER.with(|observer| *observer.borrow_mut() = Some((signal, sender)));
            7
        });
        // The worker has finished its work and entered its first idle hook.
        assert!(!idle
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap());
        let (value, credit) = pending.finish();
        assert_eq!(value, 7);
        retired.store(true, std::sync::atomic::Ordering::Release);
        drop(credit);
        assert!(idle
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap());
    });
}

#[test]
fn readiness_keeps_the_owned_result_and_credit_until_consumption() {
    with_print_pool(2, 1, 64, |pool| {
        let pool = pool.unwrap();
        let (release, wait) = mpsc::channel();
        let mut pending = pool.submit(pool.acquire(), move |_| {
            wait.recv().unwrap();
            Box::new(37)
        });
        assert!(!pending.is_ready());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !pending.is_ready() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(pending.is_ready());
        assert_eq!(pool.credits.usage.lock().unwrap().units, 1);
        let (value, credit) = pending.finish();
        assert_eq!(*value, 37);
        drop(credit);
        assert_eq!(pool.credits.usage.lock().unwrap().units, 0);
    });
}

#[test]
fn three_print_workers_cover_a_larger_spare_budget() {
    with_print_pool(16, 4, 64, |pool| {
        let pool = pool.unwrap();
        let barrier = Arc::new(Barrier::new(4));
        let jobs = (0..6)
            .map(|index| {
                let barrier = Arc::clone(&barrier);
                pool.submit(pool.acquire(), move |_| {
                    if index < 3 {
                        barrier.wait();
                    }
                    index
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let actual = jobs
            .into_iter()
            .map(|job| job.finish().0)
            .collect::<Vec<_>>();
        assert_eq!(actual, [0, 1, 2, 3, 4, 5]);
        assert_eq!(pool.peak().workers, 3);
    });
}

#[test]
fn a_small_plan_limits_printer_heaps_before_jobs_start() {
    let mut pool = EmitPrintPool::for_budget(10, 4, 64).unwrap();
    pool.set_planned_units(187);
    let barrier = Arc::new(Barrier::new(3));
    let jobs = (0..5)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            pool.submit(pool.acquire(), move |_| {
                if index < 2 {
                    barrier.wait();
                }
                index
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    assert_eq!(
        jobs.into_iter()
            .map(|job| job.finish().0)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(pool.peak().workers, 2);
}
