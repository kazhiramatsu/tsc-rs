#![forbid(unsafe_code)]

use std::any::Any;
use std::error::Error;
use std::fmt;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// A structural failure in the bounded ordered worker pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PipelineError {
    ZeroWorkers,
    WorkerSpawnFailed { worker: usize, message: String },
    ResultChannelDisconnected { received: usize, expected: usize },
    MissingResult { index: usize },
    DuplicateResult { index: usize },
    ResultIndexOutOfRange { index: usize, item_count: usize },
    WorkerPanicked { worker: usize, message: String },
}

impl fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWorkers => {
                formatter.write_str("bounded pipeline requires at least one worker")
            }
            Self::WorkerSpawnFailed { worker, message } => write!(
                formatter,
                "bounded pipeline could not spawn worker {worker}: {message}"
            ),
            Self::ResultChannelDisconnected { received, expected } => write!(
                formatter,
                "bounded pipeline result channel disconnected after {received} of {expected} results"
            ),
            Self::MissingResult { index } => {
                write!(formatter, "bounded pipeline is missing result {index}")
            }
            Self::DuplicateResult { index } => {
                write!(formatter, "bounded pipeline received duplicate result {index}")
            }
            Self::ResultIndexOutOfRange { index, item_count } => write!(
                formatter,
                "bounded pipeline result index {index} is outside item count {item_count}"
            ),
            Self::WorkerPanicked { worker, message } => {
                write!(formatter, "bounded pipeline worker {worker} panicked: {message}")
            }
        }
    }
}

impl Error for PipelineError {}

enum WorkerMessage<R> {
    Result { index: usize, value: R },
    Finished { worker: usize },
}

/// Map a borrowed input slice with bounded workers and return results in input order.
///
/// Worker `n` owns indexes `n, n + worker_count, ...`. Completion order is
/// intentionally invisible to callers: the collector stores each result in its
/// input slot and publishes only the fully ordered vector.
pub(crate) fn ordered_map<T, R, F>(
    items: &[T],
    worker_count: usize,
    map: F,
) -> Result<Vec<R>, PipelineError>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    if worker_count == 0 {
        return Err(PipelineError::ZeroWorkers);
    }
    if items.is_empty() {
        return Ok(Vec::new());
    }
    // Keep the one-worker policy bit-for-bit caller-thread serial. Besides
    // avoiding a needless stack allocation, this is the resource-safe
    // fallback for callers that disable a shared cache or run on one core.
    if worker_count == 1 {
        return Ok(items
            .iter()
            .enumerate()
            .map(|(index, item)| map(index, item))
            .collect());
    }
    thread::scope(|scope| {
        // At most `worker_count` completed messages can wait while the
        // collector is placing results into their ordered slots.
        let (sender, receiver) = mpsc::sync_channel(worker_count);
        let map = &map;
        let mut handles = Vec::with_capacity(worker_count);
        let mut spawn_error = None;

        for worker in 0..worker_count {
            let sender = sender.clone();
            let handle = thread::Builder::new()
                .name(format!("xtask-ordered-map-{worker}"))
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, move || {
                    for index in (worker..items.len()).step_by(worker_count) {
                        let value = map(index, &items[index]);
                        if sender.send(WorkerMessage::Result { index, value }).is_err() {
                            return;
                        }
                    }
                    let _ = sender.send(WorkerMessage::Finished { worker });
                });
            match handle {
                Ok(handle) => handles.push((worker, handle)),
                Err(error) => {
                    spawn_error = Some(PipelineError::WorkerSpawnFailed {
                        worker,
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
        // The receiver must observe disconnection if a worker exits before
        // its completion marker. Keeping this original sender alive would
        // turn such a failure into an indefinite receive.
        drop(sender);

        let collected = match spawn_error {
            Some(error) => Err(error),
            None => collect_results(&receiver, items.len(), worker_count),
        };
        // A protocol error stops collection early. Drop the receiver before
        // joining so workers blocked in `send` wake up and exit.
        drop(receiver);

        let mut first_panic = None;
        for (worker, handle) in handles {
            if let Err(payload) = handle.join() {
                if first_panic.is_none() {
                    first_panic = Some(PipelineError::WorkerPanicked {
                        worker,
                        message: panic_message(payload),
                    });
                }
            }
        }

        match first_panic {
            Some(error) => Err(error),
            None => collected,
        }
    })
}

fn collect_results<R>(
    receiver: &Receiver<WorkerMessage<R>>,
    item_count: usize,
    worker_count: usize,
) -> Result<Vec<R>, PipelineError> {
    let mut results = std::iter::repeat_with(|| None)
        .take(item_count)
        .collect::<Vec<Option<R>>>();
    let mut finished = vec![false; worker_count];
    let mut finished_count = 0usize;
    let mut received = 0usize;

    while finished_count < worker_count {
        let message = receiver
            .recv()
            .map_err(|_| PipelineError::ResultChannelDisconnected {
                received,
                expected: item_count,
            })?;
        match message {
            WorkerMessage::Result { index, value } => {
                let Some(slot) = results.get_mut(index) else {
                    return Err(PipelineError::ResultIndexOutOfRange { index, item_count });
                };
                if slot.is_some() {
                    return Err(PipelineError::DuplicateResult { index });
                }
                *slot = Some(value);
                received += 1;
            }
            WorkerMessage::Finished { worker } => {
                // Worker ids are produced only by the fixed spawn loop above.
                // Treat a duplicate marker as a disconnected protocol: without
                // every distinct marker the collector cannot safely complete.
                let Some(is_finished) = finished.get_mut(worker) else {
                    return Err(PipelineError::ResultChannelDisconnected {
                        received,
                        expected: item_count,
                    });
                };
                if *is_finished {
                    return Err(PipelineError::ResultChannelDisconnected {
                        received,
                        expected: item_count,
                    });
                }
                *is_finished = true;
                finished_count += 1;
            }
        }
    }

    let mut ordered = Vec::with_capacity(item_count);
    for (index, result) in results.into_iter().enumerate() {
        ordered.push(result.ok_or(PipelineError::MissingResult { index })?);
    }
    Ok(ordered)
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

#[cfg(test)]
#[path = "../tests/unit/bounded_pipeline/tests.rs"]
mod tests;
