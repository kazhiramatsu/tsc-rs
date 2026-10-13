//! Go's `flag` package as tsgo's `--api` and `--lsp` command lines use it
//! (`flag.NewFlagSet(name, flag.ContinueOnError)`): `-name` or `--name`, a
//! value after `=` or in the next argument (a boolean's only after `=`),
//! parsing up to the first argument that is not a flag.

/// A flag's value type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    String,
    Int,
}

/// A flag: its name, type and usage text.
#[derive(Clone, Copy, Debug)]
pub struct Flag {
    pub name: &'static str,
    pub kind: Kind,
    pub usage: &'static str,
}

/// A flag's parsed value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    String(String),
    Int(i64),
}

/// `FlagSet.Parse`: the flags given, in order. An error carries Go's
/// message, or none for `-h` and `-help`; either way Go prints the usage
/// after it.
pub fn parse(
    arguments: &[String],
    flags: &[Flag],
) -> Result<Vec<(&'static str, Value)>, Option<String>> {
    let mut values = Vec::new();
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        let Some(flag) = argument
            .strip_prefix("--")
            .or_else(|| argument.strip_prefix('-'))
            .filter(|flag| argument.len() > 1 && !flag.is_empty())
        else {
            break;
        };
        if flag.starts_with('-') || flag.starts_with('=') {
            return Err(Some(format!("bad flag syntax: {argument}")));
        }
        let (name, value) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let Some(known) = flags.iter().find(|known| known.name == name) else {
            if name == "help" || name == "h" {
                return Err(None);
            }
            return Err(Some(format!("flag provided but not defined: -{name}")));
        };
        if known.kind == Kind::Bool {
            let enabled = match value {
                None => true,
                Some(value) => parse_bool(value).ok_or_else(|| {
                    Some(format!(
                        "invalid boolean value {value:?} for -{name}: parse error"
                    ))
                })?,
            };
            values.push((known.name, Value::Bool(enabled)));
            continue;
        }
        let value = match value {
            Some(value) => value.to_owned(),
            None => remaining
                .next()
                .cloned()
                .ok_or_else(|| Some(format!("flag needs an argument: -{name}")))?,
        };
        let value = match known.kind {
            Kind::Int => Value::Int(parse_int(&value).map_err(|reason| {
                Some(format!(
                    "invalid value {value:?} for flag -{name}: {reason}"
                ))
            })?),
            _ => Value::String(value),
        };
        values.push((known.name, value));
    }
    Ok(values)
}

/// Go's `strconv.ParseBool`.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// Go's `strconv.ParseInt(value, 0, 64)` (an int flag on a 64-bit
/// platform): a sign, then a `0x`, `0o`, `0b` or `0` prefix choosing the
/// base, with underscores between digits. The error is the flag package's
/// reason.
fn parse_int(value: &str) -> Result<i64, &'static str> {
    let (negative, digits) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let lower = digits.to_ascii_lowercase();
    let (base, body) = match lower.as_bytes() {
        [b'0', b'b', _, ..] => (2, &lower[2..]),
        [b'0', b'o', _, ..] => (8, &lower[2..]),
        [b'0', b'x', _, ..] => (16, &lower[2..]),
        [b'0', _, ..] => (8, &lower[1..]),
        _ => (10, lower.as_str()),
    };
    if body.is_empty() {
        return Err("parse error");
    }
    let mut magnitude: u64 = 0;
    for byte in body.bytes() {
        let digit = match byte {
            b'_' => continue,
            b'0'..=b'9' => u64::from(byte - b'0'),
            b'a'..=b'z' => u64::from(byte - b'a') + 10,
            _ => return Err("parse error"),
        };
        if digit >= base {
            return Err("parse error");
        }
        magnitude = magnitude
            .checked_mul(base)
            .and_then(|value| value.checked_add(digit))
            .ok_or("value out of range")?;
    }
    if body.contains('_') && !underscores_ok(&lower) {
        return Err("parse error");
    }
    if negative {
        if magnitude > i64::MIN.unsigned_abs() {
            return Err("value out of range");
        }
        return Ok(0_i64.wrapping_sub_unsigned(magnitude));
    }
    i64::try_from(magnitude).map_err(|_| "value out of range")
}

/// Go's `underscoreOK`: each underscore follows a digit or the base prefix
/// and is followed by a digit.
fn underscores_ok(digits: &str) -> bool {
    let bytes = digits.as_bytes();
    let mut index = 0;
    // The last character's class: '^' the start, '0' a digit or the base
    // prefix, '_' an underscore, '!' anything else.
    let mut saw = b'^';
    let mut hex = false;
    if bytes.len() >= 2 && bytes[0] == b'0' && matches!(bytes[1], b'b' | b'o' | b'x') {
        index = 2;
        saw = b'0';
        hex = bytes[1] == b'x';
    }
    for &byte in &bytes[index..] {
        if byte.is_ascii_digit() || (hex && (b'a'..=b'f').contains(&byte)) {
            saw = b'0';
            continue;
        }
        if byte == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
            continue;
        }
        if saw == b'_' {
            return false;
        }
        saw = b'!';
    }
    saw != b'_'
}

/// `FlagSet.PrintDefaults` under `Usage of <set>:`, the flags sorted by
/// name; `default` gives the default a flag shows when it is not the zero
/// value (a string's is quoted).
pub fn usage(set: &str, flags: &[Flag], default: impl Fn(&str) -> Option<String>) -> String {
    let mut sorted = flags.to_vec();
    sorted.sort_by_key(|flag| flag.name);
    let mut usage = format!("Usage of {set}:\n");
    for flag in sorted {
        let mut line = format!("  -{}", flag.name);
        match flag.kind {
            Kind::Bool => {}
            Kind::String => line.push_str(" string"),
            Kind::Int => line.push_str(" int"),
        }
        // Go puts a one-letter boolean's usage on the same line.
        if line.len() <= 4 {
            line.push('\t');
        } else {
            line.push_str("\n    \t");
        }
        line.push_str(&flag.usage.replace('\n', "\n    \t"));
        if let Some(value) = default(flag.name) {
            if flag.kind == Kind::String {
                line.push_str(&format!(" (default {value:?})"));
            } else {
                line.push_str(&format!(" (default {value})"));
            }
        }
        usage.push_str(&line);
        usage.push('\n');
    }
    usage
}
