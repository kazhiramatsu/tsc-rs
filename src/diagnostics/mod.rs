//! Diagnostic model mirroring tsc: message catalog, message chains, sorting,
//! deduplication. Formatting lives in `crate::output`.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Error,
    Warning,
    Suggestion,
    Message,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Error => "error",
            Category::Warning => "warning",
            Category::Suggestion => "suggestion",
            Category::Message => "message",
        }
    }
}

#[derive(Debug)]
pub struct DiagnosticMessage {
    pub code: u32,
    pub category: Category,
    pub text: &'static str,
    pub elided: bool,
    /// tsc `reportsUnnecessary`: render as a faded/unnecessary hint (unused code).
    pub reports_unnecessary: bool,
    /// tsc `reportsDeprecated`: render with a deprecation strike-through.
    pub reports_deprecated: bool,
}

#[allow(non_upper_case_globals)]
pub mod gen {
    use super::{Category, DiagnosticMessage};
    include!(concat!(env!("OUT_DIR"), "/diagnostics_gen.rs"));
}

pub fn by_code(code: u32) -> Option<&'static DiagnosticMessage> {
    gen::ALL_BY_CODE
        .binary_search_by_key(&code, |m| m.code)
        .ok()
        .map(|i| gen::ALL_BY_CODE[i])
}

/// `{0}`/`{1}`... substitution, exactly like tsc's formatStringFromArgs.
pub fn format_message(template: &str, args: &[String]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' {
                let idx: usize = template[i + 1..j].parse().unwrap();
                out.push_str(args.get(idx).map(|s| s.as_str()).unwrap_or(""));
                i = j + 1;
                continue;
            }
        }
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// A rendered message tree (tsc DiagnosticMessageChain): text plus children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageChain {
    pub code: u32,
    pub category: Category,
    pub text: String,
    pub next: Vec<MessageChain>,
}

impl MessageChain {
    pub fn new(msg: &'static DiagnosticMessage, args: &[String]) -> MessageChain {
        MessageChain {
            code: msg.code,
            category: msg.category,
            text: format_message(msg.text, args),
            next: Vec::new(),
        }
    }
}

/// A tsc relatedInformation entry: a secondary message anchored at another
/// source location, attached to a primary diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedInfo {
    pub file: Option<usize>,
    pub start: u32,
    pub length: u32,
    pub message: MessageChain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Index into the program's file list; None = global (file-less) diagnostic.
    pub file: Option<usize>,
    /// Byte offset within the file (monotonic with tsc's UTF-16 offsets for ordering).
    pub start: u32,
    pub length: u32,
    pub message: MessageChain,
    /// tsc relatedInformation (secondary locations); empty for most diagnostics.
    pub related: Vec<RelatedInfo>,
}

impl Diagnostic {
    pub fn code(&self) -> u32 {
        self.message.code
    }
    pub fn category(&self) -> Category {
        self.message.category
    }
}

fn compare_chains(a: &MessageChain, b: &MessageChain) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // tsc compareMessageText: compare text, then a chain without children sorts
    // before one with children, then children pairwise, then child count.
    match a.text.cmp(&b.text) {
        Ordering::Equal => {}
        o => return o,
    }
    match (a.next.is_empty(), b.next.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    for (ca, cb) in a.next.iter().zip(b.next.iter()) {
        match compare_chains(ca, cb) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    a.next.len().cmp(&b.next.len())
}

/// Sort & dedupe like ts.sortAndDeduplicateDiagnostics. `paths[i]` must be the
/// canonical path used for ordering of file index `i` (lowercased absolute on
/// case-insensitive filesystems).
pub fn sort_and_dedupe(mut diags: Vec<Diagnostic>, paths: &[String]) -> Vec<Diagnostic> {
    use std::cmp::Ordering;
    let key = |d: &Diagnostic| d.file.map(|f| paths[f].clone());
    diags.sort_by(|a, b| {
        let pa = key(a);
        let pb = key(b);
        match (pa, pb) {
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => {}
                o => return o,
            },
            (None, None) => {}
        }
        if a.file.is_some() {
            match a.start.cmp(&b.start) {
                Ordering::Equal => {}
                o => return o,
            }
            match a.length.cmp(&b.length) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        match a.code().cmp(&b.code()) {
            Ordering::Equal => {}
            o => return o,
        }
        compare_chains(&a.message, &b.message)
    });
    diags.dedup();
    diags
}
