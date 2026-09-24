//! Per-source-line check profile: an interpreter's line profiler for the
//! type checker.
//!
//! With `TSRS_LINE_PROFILE=<path>` in the environment, every statement-level
//! check (`check_source_element`, deferred checks) is timed and its work
//! counted — instantiations, types created, call signatures resolved,
//! conditional types evaluated, relation checks, name resolutions, mapped
//! and indexed-access instantiations, member resolutions — and attributed
//! to the source line where the checked element starts. Lazy work forced
//! from that line (another file's types, library types) counts toward it,
//! which is the point: the report names the line that triggers the cost.
//!
//! Every checker (each shard, or the serial checker) keeps its own rows and
//! flushes them into one process-wide table; `write_report` merges the rows
//! by file and line, writes them as TSV sorted by self time, and prints the
//! heaviest lines to stderr. Self time excludes nested statements (blocks,
//! function bodies), inclusive time keeps them.
//!
//! Off by default: the enabled check is one field read per statement and the
//! operation counters are plain increments.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use rustc_hash::FxHashMap;
use tsc_syntax::SourceFile;

/// Operation counters kept on the checker state (`profile_ops`).
pub const OP_SIGNATURES: usize = 0;
pub const OP_CONDITIONAL: usize = 1;
pub const OP_RELATIONS: usize = 2;
pub const OP_RESOLVE_NAME: usize = 3;
pub const OP_MAPPED: usize = 4;
pub const OP_INDEXED: usize = 5;
pub const OP_MEMBERS: usize = 6;
pub const OPS: usize = 7;
/// The counters of one snapshot: the `OPS` operation counters, then the
/// instantiation count and the type count.
pub const COUNTERS: usize = OPS + 2;
pub const COUNTER_NAMES: [&str; COUNTERS] = [
    "signatures",
    "conditional",
    "relations",
    "resolve_name",
    "mapped",
    "indexed",
    "members",
    "instantiations",
    "types",
];

fn report_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("TSRS_LINE_PROFILE").map(|value| value.to_string_lossy().into_owned())
    })
    .as_deref()
}

/// Whether `TSRS_LINE_PROFILE` selected a report path.
pub fn enabled() -> bool {
    report_path().is_some()
}

struct Frame {
    key: (usize, u32),
    started: Instant,
    child_ms: f64,
    entry: [u64; COUNTERS],
    child_ops: [u64; COUNTERS],
}

#[derive(Clone, Copy, Default)]
struct Accumulator {
    self_ms: f64,
    inclusive_ms: f64,
    hits: u64,
    self_ops: [u64; COUNTERS],
    inclusive_ops: [u64; COUNTERS],
}

/// One merged row of the report.
#[derive(Clone, Debug)]
pub struct Row {
    pub file: String,
    pub line: u32,
    pub text: String,
    pub self_ms: f64,
    pub inclusive_ms: f64,
    pub hits: u64,
    pub self_ops: [u64; COUNTERS],
    pub inclusive_ops: [u64; COUNTERS],
}

static ROWS: Mutex<Vec<Row>> = Mutex::new(Vec::new());

/// The per-checker collector.
pub struct LineProfiler<'a> {
    enabled: bool,
    stack: Vec<Frame>,
    rows: FxHashMap<(usize, u32), Accumulator>,
    files: FxHashMap<usize, &'a SourceFile>,
}

impl<'a> LineProfiler<'a> {
    pub fn new() -> Self {
        Self {
            enabled: enabled(),
            stack: Vec::new(),
            rows: FxHashMap::default(),
            files: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Open a frame for the element starting at byte `start` of `source`.
    pub fn enter(&mut self, source: &'a SourceFile, start: usize, entry: [u64; COUNTERS]) {
        let key = source as *const SourceFile as usize;
        self.files.entry(key).or_insert(source);
        let line = source
            .positions()
            .byte_to_utf16(u32::try_from(start).unwrap_or(u32::MAX))
            .and_then(|position| source.positions().line_and_character_utf16(position))
            .map_or(0, |location| location.line);
        self.stack.push(Frame {
            key: (key, line),
            started: Instant::now(),
            child_ms: 0.0,
            entry,
            child_ops: [0; COUNTERS],
        });
    }

    /// Close the innermost frame with the counters at exit.
    pub fn exit(&mut self, exit: [u64; COUNTERS]) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        let elapsed = frame.started.elapsed().as_secs_f64() * 1e3;
        let mut inclusive_ops = [0u64; COUNTERS];
        let mut self_ops = [0u64; COUNTERS];
        for index in 0..COUNTERS {
            inclusive_ops[index] = exit[index].saturating_sub(frame.entry[index]);
            self_ops[index] = inclusive_ops[index].saturating_sub(frame.child_ops[index]);
        }
        let row = self.rows.entry(frame.key).or_default();
        row.hits += 1;
        row.inclusive_ms += elapsed;
        row.self_ms += (elapsed - frame.child_ms).max(0.0);
        for ((row_inclusive, row_self), (inclusive, own)) in row
            .inclusive_ops
            .iter_mut()
            .zip(row.self_ops.iter_mut())
            .zip(inclusive_ops.iter().zip(self_ops.iter()))
        {
            *row_inclusive += inclusive;
            *row_self += own;
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.child_ms += elapsed;
            for (child, inclusive) in parent.child_ops.iter_mut().zip(inclusive_ops.iter()) {
                *child += inclusive;
            }
        }
    }

    /// Move this checker's rows into the process-wide table.
    pub fn flush(&mut self) {
        if !self.enabled || self.rows.is_empty() {
            return;
        }
        let mut rows = Vec::with_capacity(self.rows.len());
        // Drained in key order so the report is independent of hashing.
        let mut drained = self.rows.drain().collect::<Vec<_>>();
        drained.sort_by_key(|(key, _)| *key);
        for ((file_key, line), accumulator) in drained {
            let Some(source) = self.files.get(&file_key) else {
                continue;
            };
            let text = line_text(source, line);
            rows.push(Row {
                file: source.file_name.to_string_lossy().into_owned(),
                line,
                text,
                self_ms: accumulator.self_ms,
                inclusive_ms: accumulator.inclusive_ms,
                hits: accumulator.hits,
                self_ops: accumulator.self_ops,
                inclusive_ops: accumulator.inclusive_ops,
            });
        }
        ROWS.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(rows);
    }
}

impl Default for LineProfiler<'_> {
    fn default() -> Self {
        Self::new()
    }
}

fn line_text(source: &SourceFile, line: u32) -> String {
    let text = source.text();
    let start = source
        .positions()
        .line_start_byte(line)
        .map_or(0, |start| start as usize)
        .min(text.len());
    let end = source
        .positions()
        .line_start_byte(line + 1)
        .map_or(text.len(), |next| next as usize)
        .min(text.len());
    let mut slice = text[start..end]
        .trim_end_matches(['\n', '\r'])
        .trim()
        .to_owned();
    const LIMIT: usize = 160;
    if slice.len() > LIMIT {
        let mut cut = LIMIT;
        while !slice.is_char_boundary(cut) {
            cut -= 1;
        }
        slice.truncate(cut);
        slice.push('…');
    }
    slice
}

/// Merge every flushed row by file and line, write the TSV report to the
/// `TSRS_LINE_PROFILE` path (sorted by self time, descending) and print the
/// heaviest lines to stderr. Returns the number of rows written.
pub fn write_report() -> Option<usize> {
    let path = report_path()?;
    let rows = std::mem::take(&mut *ROWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    let mut merged: FxHashMap<(String, u32), Row> = FxHashMap::default();
    for row in rows {
        let entry = merged
            .entry((row.file.clone(), row.line))
            .or_insert_with(|| Row {
                self_ms: 0.0,
                inclusive_ms: 0.0,
                hits: 0,
                self_ops: [0; COUNTERS],
                inclusive_ops: [0; COUNTERS],
                ..row.clone()
            });
        entry.self_ms += row.self_ms;
        entry.inclusive_ms += row.inclusive_ms;
        entry.hits += row.hits;
        for index in 0..COUNTERS {
            entry.self_ops[index] += row.self_ops[index];
            entry.inclusive_ops[index] += row.inclusive_ops[index];
        }
    }
    let mut rows: Vec<Row> = merged.into_values().collect();
    rows.sort_by(|a, b| {
        b.self_ms
            .partial_cmp(&a.self_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });
    let mut out = String::new();
    out.push_str("self_ms\tinclusive_ms\thits");
    for name in COUNTER_NAMES {
        out.push('\t');
        out.push_str(name);
    }
    for name in COUNTER_NAMES {
        out.push_str("\tincl_");
        out.push_str(name);
    }
    out.push_str("\tfile\tline\ttext\n");
    let total_self: f64 = rows.iter().map(|row| row.self_ms).sum();
    for row in &rows {
        out.push_str(&format!(
            "{:.3}\t{:.3}\t{}",
            row.self_ms, row.inclusive_ms, row.hits
        ));
        for value in row.self_ops {
            out.push_str(&format!("\t{value}"));
        }
        for value in row.inclusive_ops {
            out.push_str(&format!("\t{value}"));
        }
        out.push_str(&format!(
            "\t{}\t{}\t{}\n",
            row.file,
            row.line + 1,
            row.text.replace('\t', " ")
        ));
    }
    if let Err(error) = std::fs::write(path, out) {
        eprintln!("tsc-rs: line profile: cannot write {path}: {error}");
        return None;
    }
    eprintln!(
        "tsc-rs line profile: {} lines, {:.1} ms self time in statements; heaviest:",
        rows.len(),
        total_self
    );
    for row in rows.iter().take(15) {
        eprintln!(
            "  {:8.1} ms  {:5} inst  {:5} types  {}:{}  {}",
            row.self_ms,
            row.self_ops[OPS],
            row.self_ops[OPS + 1],
            row.file.rsplit('/').next().unwrap_or(&row.file),
            row.line + 1,
            &row.text[..row.text.len().min(90)]
        );
    }
    Some(rows.len())
}
