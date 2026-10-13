//! Go's `time` package as tsgo's logs print it.

use std::time::Duration;

/// Go's `time.Duration.String`: `0s`, `1.5ms`, `851.125µs`, `2m3.5s`.
pub fn duration_string(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    if nanos == 0 {
        return "0s".to_owned();
    }
    if nanos < 1_000_000_000 {
        let (unit, precision) = match nanos {
            0..1_000 => ("ns", 0),
            1_000..1_000_000 => ("µs", 3),
            _ => ("ms", 6),
        };
        let (whole, fraction) = split_fraction(nanos, precision);
        return format!("{whole}{fraction}{unit}");
    }
    let (seconds, fraction) = split_fraction(nanos, 9);
    let mut text = String::new();
    let minutes = seconds / 60;
    if minutes > 0 {
        let hours = minutes / 60;
        if hours > 0 {
            text.push_str(&format!("{hours}h"));
        }
        text.push_str(&format!("{}m", minutes % 60));
    }
    text.push_str(&format!("{}{fraction}s", seconds % 60));
    text
}

/// Go's `fmtFrac`: `value / 10^precision` and its fraction without
/// trailing zeros (and without the point when it is zero).
fn split_fraction(value: u128, precision: u32) -> (u128, String) {
    let scale = 10_u128.pow(precision);
    let fraction = value % scale;
    if fraction == 0 {
        return (value / scale, String::new());
    }
    let digits = format!("{fraction:0width$}", width = precision as usize);
    (value / scale, format!(".{}", digits.trim_end_matches('0')))
}
