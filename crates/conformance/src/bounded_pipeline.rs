//! Bounded ordered streaming for the conformance corpus runner.
//!
//! `ordered_for_each` runs `map` over the items on a bounded scoped worker
//! pool while the caller's `consume` observes every result strictly in item
//! order on the calling thread. Retention stays bounded independently of
//! corpus size: workers block once the result channel and the reorder
//! buffer hold roughly three results per worker, so a slow consumer
//! backpressures the checkers instead of accumulating output.
//!
//! This deliberately mirrors the discipline of the xtask
//! `bounded_pipeline::ordered_map` (which is pinned as hosted acceptance
//! call-graph evidence and therefore stays in place) while adding the
//! streaming consume the grading pipeline needs.
//! tsrs-native: gate-infrastructure concurrency utility; no tsc counterpart.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub(crate) fn ordered_for_each<T, R>(
    items: &[T],
    workers: usize,
    map: impl Fn(usize, &T) -> R + Sync,
    mut consume: impl FnMut(usize, R) -> Result<(), String>,
) -> Result<(), String>
where
    T: Sync,
    R: Send,
{
    if workers == 0 {
        return Err("ordered_for_each requires at least one worker".to_owned());
    }
    if items.is_empty() {
        return Ok(());
    }
    let worker_count = workers.min(items.len());
    let next_item = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::sync_channel::<(usize, R)>(worker_count.saturating_mul(2));

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let sender = sender.clone();
            let next_item = &next_item;
            let map = &map;
            handles.push(scope.spawn(move || loop {
                let index = next_item.fetch_add(1, Ordering::Relaxed);
                if index >= items.len() {
                    break;
                }
                let result = map(index, &items[index]);
                if sender.send((index, result)).is_err() {
                    // The consumer stopped early; unclaimed items are
                    // deliberately abandoned.
                    break;
                }
            }));
        }
        drop(sender);

        let mut reorder = BTreeMap::new();
        let mut expected = 0usize;
        let mut consume_error = None;
        while expected < items.len() && consume_error.is_none() {
            let Ok((index, result)) = receiver.recv() else {
                break;
            };
            reorder.insert(index, result);
            while let Some(result) = reorder.remove(&expected) {
                match consume(expected, result) {
                    Ok(()) => expected += 1,
                    Err(error) => {
                        consume_error = Some(error);
                        break;
                    }
                }
            }
        }
        // Dropping the receiver unblocks any worker parked on a full
        // channel so the scope can join.
        drop(receiver);

        let mut panic_message = None;
        for handle in handles {
            if let Err(payload) = handle.join() {
                let message = payload
                    .downcast_ref::<&'static str>()
                    .map(|text| (*text).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string worker panic payload".to_owned());
                panic_message.get_or_insert(message);
            }
        }
        if let Some(message) = panic_message {
            return Err(format!("ordered_for_each worker panicked: {message}"));
        }
        if let Some(error) = consume_error {
            return Err(error);
        }
        if expected < items.len() {
            return Err(format!(
                "ordered_for_each result stream ended after {expected} of {} items",
                items.len()
            ));
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "../tests/unit/bounded_pipeline/tests.rs"]
mod tests;
