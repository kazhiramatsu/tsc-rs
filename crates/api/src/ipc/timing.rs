//! tsgo `ipc/timing.go`: the server's per-request processing time, which a
//! client reads with `getServerTiming`.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tsc_types::js_number_to_string;

/// The meta-requests the connection answers itself.
pub(super) const METHOD_GET_SERVER_TIMING: &str = "getServerTiming";
pub(super) const METHOD_RESET_SERVER_TIMING: &str = "resetServerTiming";

/// tsgo `serverRecentRequestCapacity`.
const RECENT_REQUEST_CAPACITY: usize = 5;

/// tsgo `serverRequestTiming`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RequestTiming {
    pub(super) method: String,
    pub(super) processing_time_ms: f64,
    pub(super) timestamp: i64,
}

/// tsgo `serverTimingTotals`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct TimingTotals {
    pub(super) request_count: u64,
    pub(super) total_processing_time_ms: f64,
}

/// tsgo `serverTimingInfo`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TimingInfo {
    pub(super) enabled: bool,
    pub(super) totals: TimingTotals,
    pub(super) recent_requests: Vec<RequestTiming>,
}

impl TimingInfo {
    /// The JSON tsgo writes, a `float64` as Go writes it (JavaScript's
    /// number text).
    pub(super) fn to_json(&self) -> String {
        let quote = |text: &str| serde_json::to_string(text).expect("a string serializes");
        let recent = self
            .recent_requests
            .iter()
            .map(|request| {
                format!(
                    r#"{{"method":{},"processingTimeMs":{},"timestamp":{}}}"#,
                    quote(&request.method),
                    js_number_to_string(request.processing_time_ms),
                    request.timestamp
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"enabled":{},"totals":{{"requestCount":{},"totalProcessingTimeMs":{}}},"recentRequests":[{recent}]}}"#,
            self.enabled,
            self.totals.request_count,
            js_number_to_string(self.totals.total_processing_time_ms)
        )
    }
}

#[derive(Debug, Default)]
struct Collected {
    totals: TimingTotals,
    /// Up to the capacity; once full, `head` is the oldest entry.
    ring: Vec<RequestTiming>,
    head: usize,
}

/// tsgo `timingCollector`.
#[derive(Debug, Default)]
pub(super) struct TimingCollector {
    collected: Mutex<Collected>,
}

impl TimingCollector {
    pub(super) fn record(&self, method: &str, duration: Duration) {
        let processing_time_ms = duration_to_millis(duration);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            });
        let mut collected = self.lock();
        collected.totals.request_count += 1;
        collected.totals.total_processing_time_ms += processing_time_ms;
        let entry = RequestTiming {
            method: method.to_owned(),
            processing_time_ms,
            timestamp,
        };
        if collected.ring.len() < RECENT_REQUEST_CAPACITY {
            collected.ring.push(entry);
        } else {
            let head = collected.head;
            collected.ring[head] = entry;
            collected.head = (head + 1) % RECENT_REQUEST_CAPACITY;
        }
    }

    /// The collected timing, recent requests oldest first.
    pub(super) fn snapshot(&self) -> TimingInfo {
        let collected = self.lock();
        let length = collected.ring.len();
        TimingInfo {
            enabled: true,
            totals: collected.totals.clone(),
            recent_requests: (0..length)
                .map(|index| collected.ring[(collected.head + index) % length].clone())
                .collect(),
        }
    }

    pub(super) fn reset(&self) {
        *self.lock() = Collected::default();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Collected> {
        self.collected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// tsgo `serverTimingSnapshot`: the collector's timing, or a disabled
/// snapshot without one.
pub(super) fn server_timing_snapshot(collector: Option<&TimingCollector>) -> TimingInfo {
    collector.map_or_else(
        || TimingInfo {
            enabled: false,
            totals: TimingTotals::default(),
            recent_requests: Vec::new(),
        },
        TimingCollector::snapshot,
    )
}

/// tsgo `durationToMillis`: fractional milliseconds from the nanoseconds.
fn duration_to_millis(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1_000_000.0
}

#[cfg(test)]
#[path = "../../tests/unit/timing.rs"]
mod tests;
