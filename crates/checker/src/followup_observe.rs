//! Temporary observation only. Keys identify repeated explicit arguments,
//! not equivalent checker state or a legal memoization opportunity.
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rustc_hash::FxHashMap;

type Key = (&'static str, [u64; 8]);

#[derive(Default)]
struct Row {
    calls: u64,
    repeats: u64,
    inclusive_ns: u64,
    self_ns: u64,
    repeat_ns: u64,
    creations: [u64; 4],
    own_creations: [u64; 4],
}

struct Frame {
    key: Key,
    repeated: bool,
    start: Instant,
    before: [u64; 4],
    child_ns: u64,
    child_creations: [u64; 4],
}

#[derive(Default)]
pub(crate) struct FollowupObserve {
    path: Option<PathBuf>,
    rows: FxHashMap<Key, Row>,
    stack: Vec<Frame>,
    counters: FxHashMap<&'static str, u64>,
}

impl FollowupObserve {
    pub(crate) fn new() -> Self {
        Self {
            path: std::env::var_os("TSRS_FOLLOWUP_COUNTERS").map(PathBuf::from),
            rows: FxHashMap::default(),
            stack: Vec::new(),
            counters: FxHashMap::default(),
        }
    }

    pub(crate) fn begin(&mut self, category: &'static str, key: [u64; 8], before: [u64; 4]) {
        if self.path.is_none() {
            return;
        }
        let key = (category, key);
        let row = self.rows.entry(key).or_default();
        let repeated = row.calls > 0;
        row.calls += 1;
        row.repeats += u64::from(repeated);
        self.stack.push(Frame {
            key,
            repeated,
            start: Instant::now(),
            before,
            child_ns: 0,
            child_creations: [0; 4],
        });
    }

    pub(crate) fn end(&mut self, after: [u64; 4]) {
        if self.path.is_none() {
            return;
        }
        let frame = self.stack.pop().expect("balanced observation scopes");
        let elapsed = frame.start.elapsed().as_nanos() as u64;
        let row = self.rows.get_mut(&frame.key).unwrap();
        row.inclusive_ns += elapsed;
        row.self_ns += elapsed.saturating_sub(frame.child_ns);
        if frame.repeated {
            row.repeat_ns += elapsed;
        }
        let mut created = [0; 4];
        for i in 0..4 {
            created[i] = after[i].saturating_sub(frame.before[i]);
            row.creations[i] += created[i];
            row.own_creations[i] += created[i].saturating_sub(frame.child_creations[i]);
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.child_ns += elapsed;
            for (child, value) in parent.child_creations.iter_mut().zip(created) {
                *child += value;
            }
        }
    }

    pub(crate) fn bump(&mut self, name: &'static str, amount: u64) {
        if self.path.is_some() {
            *self.counters.entry(name).or_default() += amount;
        }
    }
}

impl Drop for FollowupObserve {
    fn drop(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        if self.rows.is_empty() {
            return;
        }
        assert!(self.stack.is_empty(), "unclosed observation scope");
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::fs::create_dir_all(path).expect("observation directory");
        let mut out = std::io::BufWriter::new(
            std::fs::File::create(path.join(format!(
                "checker-{}-{}.tsv",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
            .expect("observation report"),
        );
        writeln!(out, "category\tkey\tcalls\trepeats\tinclusive_ns\tself_ns\trepeat_ns\ttypes\tsymbols\tsignatures\tinstantiations\tself_types\tself_symbols\tself_signatures\tself_instantiations").unwrap();
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by(|a, b| {
            b.1.inclusive_ns
                .cmp(&a.1.inclusive_ns)
                .then_with(|| a.0.cmp(b.0))
        });
        for ((category, key), row) in rows {
            write!(
                out,
                "{category}\t{key:?}\t{}\t{}\t{}\t{}\t{}",
                row.calls, row.repeats, row.inclusive_ns, row.self_ns, row.repeat_ns
            )
            .unwrap();
            for value in row.creations.iter().chain(row.own_creations.iter()) {
                write!(out, "\t{value}").unwrap();
            }
            writeln!(out).unwrap();
        }
        let mut counters: Vec<_> = self.counters.iter().collect();
        counters.sort_by_key(|(name, _)| *name);
        for (name, value) in counters {
            writeln!(out, "counter.{name}\t[]\t{value}").unwrap();
        }
    }
}

impl crate::state::CheckerState<'_> {
    pub(crate) fn followup_snapshot(&self) -> [u64; 4] {
        [
            self.tables.len() as u64,
            self.binder.transient_symbol_count() as u64,
            self.signatures.len() as u64,
            self.total_instantiation_count,
        ]
    }
}
