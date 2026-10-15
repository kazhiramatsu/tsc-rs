//! tsgo `lsp/stack_sanitizer.go`: a panic's stack for telemetry with
//! nothing of the user's — tsc-rs's own frames kept (their functions, and
//! their source files from the repository's `crates` directory), every other
//! frame redacted — and VS Code's generic secret pattern defeated.

use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::sync::Once;

thread_local! {
    /// The stack of the thread's last panic (tsgo's recovering `debug.Stack`
    /// sees the panicking frames; a Rust stack is gone once the panic is
    /// caught, so the panic hook keeps it).
    static PANIC_STACK: RefCell<Option<Backtrace>> = const { RefCell::new(None) };
}

/// Keeps each panic's stack for [`take_panic_stack`], the process's panic
/// hook otherwise unchanged.
pub(crate) fn capture_panic_stacks() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANIC_STACK.with(|stack| *stack.borrow_mut() = Some(Backtrace::force_capture()));
            previous(info);
        }));
    });
}

/// The stack of the thread's last panic, once.
pub(crate) fn take_panic_stack() -> String {
    PANIC_STACK
        .with(|stack| stack.borrow_mut().take())
        .map(|stack| stack.to_string())
        .unwrap_or_default()
}

/// tsgo `sanitizeStackTrace` over `std::backtrace`'s text: a frame's
/// function line and its `at` line, each tsc-rs's or `(REDACTED FRAME)`.
pub fn sanitize_stack_trace(stack: &str) -> String {
    let mut result = String::new();
    let mut ours = false;
    for (index, line) in stack.lines().enumerate() {
        if index > 0 {
            result.push('\n');
        }
        let line = line.trim();
        if let Some(location) = line.strip_prefix("at ") {
            result.push('\t');
            match ours.then(|| sanitized_location(location)).flatten() {
                Some(location) => result.push_str(&location),
                None => result.push_str("(REDACTED FRAME)"),
            }
            continue;
        }
        let function = line
            .split_once(": ")
            .filter(|(number, _)| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
            .map_or(line, |(_, function)| function);
        ours = is_ours(function);
        if ours {
            result.push_str(function);
        } else {
            result.push_str("(REDACTED FRAME)");
        }
    }
    defeat_generic_secret_regex(&result)
}

/// A function of tsc-rs's crates (`tsc_*`), also as a trait impl's type.
fn is_ours(function: &str) -> bool {
    function.trim_start_matches('<').starts_with("tsc_")
}

/// A source file of the repository as `crates|>…|>file.rs:line` (tsgo's
/// path segments), without the column.
fn sanitized_location(location: &str) -> Option<String> {
    let start = location.rfind("crates/")?;
    let path = &location[start..];
    let path = match path.rsplit_once(':') {
        Some((path, column))
            if column.bytes().all(|b| b.is_ascii_digit()) && path.contains(':') =>
        {
            path
        }
        _ => path,
    };
    Some(path.replace('/', "|>"))
}

/// tsgo `defeatGenericSecretRegex`: VS Code's telemetry redacts any text
/// matching `(key|token|sig|secret|signature|password|passwd|pwd|
/// android:value)[^a-zA-Z0-9]`; `X_X` goes after each trigger keyword
/// (`key`, `token`, `signature`, `sig`, `pwd`, any case) followed by `(`,
/// `[`, `.` or `|`.
pub fn defeat_generic_secret_regex(text: &str) -> String {
    const KEYWORDS: [&str; 5] = ["key", "token", "signature", "sig", "pwd"];
    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let matched = KEYWORDS.iter().find(|keyword| {
            let end = index + keyword.len();
            end < bytes.len()
                && bytes[index..end].eq_ignore_ascii_case(keyword.as_bytes())
                && matches!(bytes[end], b'(' | b'[' | b'.' | b'|')
        });
        match matched {
            Some(keyword) => {
                let end = index + keyword.len();
                result.push_str(&text[index..end]);
                result.push_str("X_X");
                result.push(char::from(bytes[end]));
                index = end + 1;
            }
            None => {
                let character = text[index..].chars().next().expect("a character");
                result.push(character);
                index += character.len_utf8();
            }
        }
    }
    result
}
