//! tsgo `ipc/timing_test.go`. A `Duration` cannot be negative, so tsgo's
//! clamp of a negative duration has no case here.

use std::time::Duration;

use super::*;

#[test]
fn totals_accumulate_and_recent_requests_are_recorded() {
    // TestTimingCollector: accumulates totals and records recent requests.
    let collector = TimingCollector::default();
    collector.record("getSourceFile", Duration::from_millis(2));
    collector.record("getSymbolAtPosition", Duration::from_micros(500));
    let snapshot = collector.snapshot();
    assert!(snapshot.enabled);
    assert_eq!(snapshot.totals.request_count, 2);
    assert_eq!(snapshot.totals.total_processing_time_ms, 2.5);
    assert_eq!(snapshot.recent_requests.len(), 2);
    assert_eq!(snapshot.recent_requests[0].method, "getSourceFile");
    assert_eq!(snapshot.recent_requests[0].processing_time_ms, 2.0);
    assert_eq!(snapshot.recent_requests[1].method, "getSymbolAtPosition");
    assert_eq!(snapshot.recent_requests[1].processing_time_ms, 0.5);
}

#[test]
fn the_ring_keeps_the_latest_requests_oldest_first() {
    // TestTimingCollector: ring buffer retains only the most recent
    // requests, oldest to newest.
    let collector = TimingCollector::default();
    let methods = ["a", "b", "c", "d", "e", "f", "g"];
    for method in methods {
        collector.record(method, Duration::from_millis(1));
    }
    let snapshot = collector.snapshot();
    assert_eq!(snapshot.totals.request_count, 7);
    let recent = snapshot
        .recent_requests
        .iter()
        .map(|request| request.method.as_str())
        .collect::<Vec<_>>();
    assert_eq!(recent, methods[methods.len() - RECENT_REQUEST_CAPACITY..]);
}

#[test]
fn a_connection_without_timing_reports_it_disabled() {
    // TestServerTimingSnapshotDisabled.
    let snapshot = server_timing_snapshot(None);
    assert!(!snapshot.enabled);
    assert_eq!(snapshot.totals.request_count, 0);
    assert!(snapshot.recent_requests.is_empty());
    assert_eq!(
        snapshot.to_json(),
        r#"{"enabled":false,"totals":{"requestCount":0,"totalProcessingTimeMs":0},"recentRequests":[]}"#
    );
}

#[test]
fn a_reset_clears_the_collector_which_keeps_working() {
    // TestTimingCollectorReset.
    let collector = TimingCollector::default();
    collector.record("a", Duration::from_millis(1));
    collector.record("b", Duration::from_millis(1));
    collector.reset();
    let snapshot = collector.snapshot();
    assert!(snapshot.enabled);
    assert_eq!(snapshot.totals.request_count, 0);
    assert_eq!(snapshot.totals.total_processing_time_ms, 0.0);
    assert!(snapshot.recent_requests.is_empty());
    collector.record("c", Duration::from_millis(2));
    let snapshot = collector.snapshot();
    assert_eq!(snapshot.totals.request_count, 1);
    assert_eq!(snapshot.recent_requests[0].method, "c");
}

#[test]
fn durations_are_fractional_milliseconds() {
    // TestDurationToMillis.
    assert_eq!(duration_to_millis(Duration::from_micros(1500)), 1.5);
    assert_eq!(duration_to_millis(Duration::ZERO), 0.0);
    assert_eq!(duration_to_millis(Duration::from_nanos(500)), 0.0005);
    assert_eq!(duration_to_millis(Duration::from_nanos(1234)), 0.001234);
}

#[test]
fn the_timing_is_written_with_go_numbers() {
    // Go writes a whole float64 without a fraction.
    let info = TimingInfo {
        enabled: true,
        totals: TimingTotals {
            request_count: 2,
            total_processing_time_ms: 2.0,
        },
        recent_requests: vec![RequestTiming {
            method: "ping".to_owned(),
            processing_time_ms: 0.0005,
            timestamp: 1,
        }],
    };
    assert_eq!(
        info.to_json(),
        r#"{"enabled":true,"totals":{"requestCount":2,"totalProcessingTimeMs":2},"recentRequests":[{"method":"ping","processingTimeMs":0.0005,"timestamp":1}]}"#
    );
}
