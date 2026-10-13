//! Go's `time.Duration.String` (`tsc_host::go_time`), with the values of
//! Go's own `durationTests` (time/time_test.go).

use std::time::Duration;

use tsc_host::go_time::duration_string;

#[test]
fn durations_print_as_go_prints_them() {
    let cases = [
        (0, "0s"),
        (1, "1ns"),
        (1_100, "1.1µs"),
        (2_200_000, "2.2ms"),
        (3_300_000_000, "3.3s"),
        (4 * 60_000_000_000 + 5_000_000_000, "4m5s"),
        (4 * 60_000_000_000 + 5_001_000_000, "4m5.001s"),
        (
            5 * 3_600_000_000_000 + 6 * 60_000_000_000 + 7_001_000_000,
            "5h6m7.001s",
        ),
        (8 * 60_000_000_000 + 1, "8m0.000000001s"),
        (3_600_000_000_000, "1h0m0s"),
        (851_125, "851.125µs"),
    ];
    for (nanos, expected) in cases {
        assert_eq!(duration_string(Duration::from_nanos(nanos)), expected);
    }
}
