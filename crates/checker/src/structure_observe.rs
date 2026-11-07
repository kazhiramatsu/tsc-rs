//! Experimental census. Explicit identities are not a memoization proof.
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use rustc_hash::FxHashMap;

type Key = (&'static str, [u64; 12]);

#[derive(Default)]
struct Row {
    calls: u64,
    ns: u64,
    own_ns: u64,
    creations: [u64; 4],
    own_creations: [u64; 4],
    fresh_results: u64,
}

struct Frame {
    category: &'static str,
    inputs: [u64; 8],
    key: Key,
    start: Instant,
    before: [u64; 4],
    child_ns: u64,
    child_creations: [u64; 4],
    arguments: FxHashMap<u64, u64>,
}

#[derive(Default)]
pub(crate) struct StructureObserve {
    directory: Option<PathBuf>,
    reported: AtomicBool,
    rows: FxHashMap<Key, Row>,
    stack: Vec<Frame>,
    counts: FxHashMap<&'static str, u64>,
    argument_histogram: FxHashMap<u64, u64>,
}

impl StructureObserve {
    pub(crate) fn new() -> Self {
        let mut observer = Self::default();
        observer.directory = std::env::var_os("TSRS_STRUCTURE_COUNTERS").map(PathBuf::from);
        observer
    }

    pub(crate) fn begin(&mut self, category: &'static str, inputs: [u64; 8], before: [u64; 4]) {
        if self.directory.is_none() {
            return;
        }
        let mut key = [0; 12];
        key[..8].copy_from_slice(&inputs);
        if let Some(call) = self.stack.iter_mut().rev().find(|f| f.category == "call") {
            key[8] = call.inputs[0];
            if category == "argument" {
                *call.arguments.entry(inputs[0]).or_default() += 1;
            }
        }
        if let Some(phase) = self
            .stack
            .iter()
            .rev()
            .find(|f| matches!(f.category, "infer_args" | "applicability"))
        {
            key[9] = phase.inputs[1];
            key[10] = u64::from(phase.category == "applicability") + 1;
        }
        if let Some(choice) = self.stack.iter().rev().find(|f| f.category == "choose") {
            key[11] = choice.inputs[1];
        }
        // Aggregate broad scopes; keep argument/literal/inference-pair detail.
        if matches!(
            category,
            "call" | "choose" | "trial" | "infer_args" | "applicability" | "flow"
        ) {
            key = [0; 12];
        }
        let key = (category, key);
        self.rows.entry(key).or_default().calls += 1;
        self.stack.push(Frame {
            category,
            inputs,
            key,
            start: Instant::now(),
            before,
            child_ns: 0,
            child_creations: [0; 4],
            arguments: FxHashMap::default(),
        });
    }

    pub(crate) fn end(&mut self, after: [u64; 4], result: Option<u32>) {
        if self.directory.is_none() {
            return;
        }
        let frame = self.stack.pop().expect("balanced census scopes");
        let elapsed = frame.start.elapsed().as_nanos() as u64;
        let row = self.rows.get_mut(&frame.key).unwrap();
        row.ns += elapsed;
        row.own_ns += elapsed.saturating_sub(frame.child_ns);
        row.fresh_results += u64::from(result.is_some_and(|r| u64::from(r) >= frame.before[0]));
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
        if frame.category == "call" {
            self.bump("call.count", 1);
            for visits in frame.arguments.into_values() {
                self.bump("call.argument_nodes", 1);
                self.bump("call.argument_checks", visits);
                self.bump("call.redundant_argument_entries", visits.saturating_sub(1));
                *self.argument_histogram.entry(visits).or_default() += 1;
            }
        }
    }

    pub(crate) fn bump(&mut self, name: &'static str, amount: u64) {
        if self.directory.is_some() {
            *self.counts.entry(name).or_default() += amount;
        }
    }

    pub(crate) fn report(&self) {
        let Some(directory) = &self.directory else {
            return;
        };
        if self.reported.swap(true, Ordering::Relaxed) {
            return;
        }
        assert!(self.stack.is_empty());
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::fs::create_dir_all(directory).unwrap();
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(directory.join(format!(
                "checker-{}-{}.tsv",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
            .unwrap(),
        );
        writeln!(file, "category\tkey\tcalls\tinclusive_ns\tself_ns\ttypes\tsymbols\tsignatures\tinstantiations\tself_types\tself_symbols\tself_signatures\tself_instantiations\tfresh_results").unwrap();
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        for ((category, key), row) in rows {
            write!(
                file,
                "{category}\t{key:?}\t{}\t{}\t{}",
                row.calls, row.ns, row.own_ns
            )
            .unwrap();
            for count in row.creations.iter().chain(row.own_creations.iter()) {
                write!(file, "\t{count}").unwrap();
            }
            writeln!(file, "\t{}", row.fresh_results).unwrap();
        }
        let mut histogram: Vec<_> = self.argument_histogram.iter().collect();
        histogram.sort_by_key(|(visits, _)| **visits);
        for (visits, count) in histogram {
            writeln!(file, "counter.call.argument_visits.{visits}\t[]\t{count}").unwrap();
        }
        let mut counts: Vec<_> = self.counts.iter().collect();
        counts.sort_by_key(|(name, _)| *name);
        for (name, count) in counts {
            writeln!(file, "counter.{name}\t[]\t{count}").unwrap();
        }
    }
}

impl Drop for StructureObserve {
    fn drop(&mut self) {
        self.report();
    }
}

impl crate::state::CheckerState<'_> {
    pub(crate) fn structure_snapshot(&self) -> [u64; 4] {
        [
            self.tables.len() as u64,
            self.binder.transient_symbol_count() as u64,
            self.signatures.len() as u64,
            self.total_instantiation_count,
        ]
    }
}
