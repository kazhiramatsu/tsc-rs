//! Go's text of the values tsgo's server logs: `fmt`'s `%v` and `%T` of a
//! JSON value decoded into `any`, and `time.Duration.String`.

use std::time::Duration;

use serde_json::Value;

/// Go's `%v` (and `%+v`) of a JSON value decoded into `any`: a map with its
/// keys sorted, `map[key:value ...]`, a slice `[a b]`, a string without
/// quotes, a number as a float64 and null as `<nil>`.
pub fn format_any(value: &Value) -> String {
    let mut text = String::new();
    write_any(&mut text, value);
    text
}

fn write_any(text: &mut String, value: &Value) {
    match value {
        Value::Null => text.push_str("<nil>"),
        Value::Bool(value) => text.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => text.push_str(&format_float(number.as_f64().unwrap_or_default())),
        Value::String(value) => text.push_str(value),
        Value::Array(items) => {
            text.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push(' ');
                }
                write_any(text, item);
            }
            text.push(']');
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            text.push_str("map[");
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    text.push(' ');
                }
                text.push_str(key);
                text.push(':');
                write_any(text, item);
            }
            text.push(']');
        }
    }
}

/// Go's `%T` of a JSON value decoded into `any`.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "<nil>",
        Value::Bool(_) => "bool",
        Value::Number(_) => "float64",
        Value::String(_) => "string",
        Value::Array(_) => "[]interface {}",
        Value::Object(_) => "map[string]interface {}",
    }
}

/// Go's `strconv.FormatFloat(value, 'g', -1, 64)` (`%v` of a float64): the
/// shortest digits, with an exponent below 1e-4 and from 1e+06.
pub fn format_float(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    // Rust's `{:e}` writes the shortest digits as `d.ddde<exp>`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent = exponent.parse::<i32>().unwrap_or_default();
    let digits = mantissa.replace('.', "");
    let sign = if value < 0.0 { "-" } else { "" };
    if !(-4..6).contains(&exponent) {
        let mut text = format!("{sign}{}", &digits[..1]);
        if digits.len() > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{text}e{exponent_sign}{:02}", exponent.unsigned_abs());
    }
    if exponent < 0 {
        let zeros = "0".repeat(exponent.unsigned_abs() as usize - 1);
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = exponent as usize + 1;
    if digits.len() <= point {
        return format!("{sign}{digits}{}", "0".repeat(point - digits.len()));
    }
    format!("{sign}{}.{}", &digits[..point], &digits[point..])
}

/// Go's `time.Duration.String`: `0s`, `1.5ms`, `851.125µs`, `2m3.5s`.
pub fn format_duration(duration: Duration) -> String {
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
