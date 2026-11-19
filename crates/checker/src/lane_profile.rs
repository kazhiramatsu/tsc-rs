//! A lane profile (tsrs-native, opt-in): what the check of every file of a
//! Program cost its checker in an earlier run, so that the next run can
//! level its checkers by what the files cost.
//!
//! The dealing of the default pool balances the files' SIZES (tsgo's
//! association: parse nodes and text bytes), and a file's size says little
//! about what its check costs: on the bench projects the longest checker
//! works 1.1 to 1.6 times the mean. A cost that was measured once is a far
//! better weight, and levelling by it needs nothing while the check runs:
//! the files are dealt as in the reference, and files then leave the
//! costliest checker only while it stands out
//! ([`crate::shard::level_by_cost`]).
//!
//! A cost is a count, never a time: the file's static part (so much per file
//! and per parse node) plus what its checker counted during its check — the
//! seven operation counters every checker keeps (the line profile's) and the
//! records of its symbol-links tables, weighted ([`counted`]). The profile of
//! a Program is therefore the same on every machine and in every run, and a
//! Program and its profile give one output. Nothing is counted for it that
//! the checker did not count before.
//!
//! The weights are a non-negative least-squares fit of every file's check
//! time on these counts, over eight projects checked by the default pool of
//! four checkers (the per-take census of 2026-11-18: 26,900 files' checks in
//! each of 48 runs, every run scaled to its own speed); the fit leaves 8 % of
//! the squared time unexplained. Only the weights' ratios matter: the unit is
//! a millionth of the static part of a file.
//! An entry is used only while the file's text has the length it was
//! recorded for, and is replaced only then — a stable Program keeps its
//! profile and its dealing. A file without an entry weighs its static part,
//! scaled by what the recorded files cost per unit of theirs.
//!
//! A profile is text, and whoever runs the check keeps it (the command line
//! names its file with `TSRS_LANE_PROFILE`): [`LaneProfile::parse`] before the
//! check, [`LaneProfile::take_update`] after it. A check without a profile
//! measures nothing, and its dealing is the reference's.

use std::fmt::Write as _;
use std::sync::Mutex;

use rustc_hash::FxHashMap;

/// What one count of each of a checker's operation counters costs, in the
/// order of the line profile: signatures resolved, conditional types,
/// relations computed, names resolved, mapped types instantiated, indexed
/// accesses, members resolved.
const OP_WEIGHTS: [u64; crate::line_profile::OPS] = [46_264, 2_784, 1_534, 3_947, 495, 2_091, 805];

/// What a record of the symbol-links tables costs: a checker that meets a
/// symbol for the first time pays for it.
const SYMBOL_LINKS_RECORD: u64 = 5_113;

/// The static part of a type-checked file: per file and per parse node.
const STATIC_PER_FILE: u64 = 1_000_000;
const STATIC_PER_NODE: u64 = 1_315;

/// The part of a type-checked file's cost that its size gives.
pub(crate) fn static_part(parse_nodes: u64) -> u64 {
    STATIC_PER_FILE.saturating_add(parse_nodes.saturating_mul(STATIC_PER_NODE))
}

/// What a checker has counted of its work so far, in cost units. The
/// difference over a file's check is what the check counted.
pub(crate) fn counted(state: &crate::state::CheckerState<'_>) -> u64 {
    state.profile_ops.iter().zip(OP_WEIGHTS).fold(
        (state.links.symbol_len() as u64).wrapping_mul(SYMBOL_LINKS_RECORD),
        |sum, (&count, weight)| sum.wrapping_add(count.wrapping_mul(weight)),
    )
}
const HEADER: &str = "# tsc-rs lane profile 1: work-clock cost, text length, file";

/// The recorded costs of a Program's files and, after a check, what there is
/// to record anew.
pub struct LaneProfile {
    /// By file name: the cost and the text length it was recorded for.
    entries: FxHashMap<String, (u64, u64)>,
    /// The record's new text when the last check changed it, until taken.
    update: Mutex<Option<String>>,
}

impl std::fmt::Debug for LaneProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LaneProfile")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// A profile is the one object its owner made: the pool's settings that carry
/// it are equal when they carry the same one.
impl PartialEq for LaneProfile {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for LaneProfile {}

impl LaneProfile {
    /// The profile a record's text holds. A line that is not an entry is
    /// none: an empty, foreign or damaged text gives fewer entries, and the
    /// check after it records them again.
    pub fn parse(text: &str) -> Self {
        let mut entries = FxHashMap::default();
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut fields = line.splitn(3, '\t');
            let (Some(cost), Some(length), Some(name)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let (Ok(cost), Ok(length)) = (cost.parse(), length.parse()) else {
                continue;
            };
            entries.insert(name.to_owned(), (cost, length));
        }
        Self {
            entries,
            update: Mutex::new(None),
        }
    }

    /// The record's new text, once, when the last check had something to
    /// record: a Program whose files keep their lengths keeps its record.
    pub fn take_update(&self) -> Option<String> {
        self.update
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Whether an earlier run has recorded anything.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The recorded cost of the file `name` whose text is `length` bytes
    /// long; none when the file was not recorded or has changed its length.
    pub(crate) fn cost(&self, name: &str, length: usize) -> Option<u64> {
        self.entries
            .get(name)
            .filter(|&&(_, recorded)| recorded == length as u64)
            .map(|&(cost, _)| cost)
    }

    /// The weights of a Program's files for the dealing: the recorded cost of
    /// a file that has one, and for any other file its static part scaled by
    /// what the recorded files cost per unit of theirs (`statics[file]` is
    /// zero for a file that is not type checked: it weighs one).
    pub(crate) fn weights(
        &self,
        names: &[String],
        lengths: &[usize],
        statics: &[u64],
    ) -> Vec<usize> {
        let recorded = names
            .iter()
            .zip(lengths)
            .map(|(name, &length)| self.cost(name, length))
            .collect::<Vec<_>>();
        // What the recorded files cost, and the static parts they have now.
        let (recorded_cost, recorded_static) = recorded
            .iter()
            .zip(statics)
            .filter_map(|(cost, &part)| cost.map(|cost| (cost, part)))
            .fold((0u128, 0u128), |(costs, parts), (cost, part)| {
                (costs + u128::from(cost), parts + u128::from(part))
            });
        recorded
            .iter()
            .zip(statics)
            .map(|(&cost, &part)| {
                let weight = match cost {
                    Some(cost) => u128::from(cost),
                    None if recorded_cost > 0 && recorded_static > 0 => {
                        u128::from(part) * recorded_cost / recorded_static
                    }
                    None => u128::from(part),
                };
                usize::try_from(weight).unwrap_or(usize::MAX).max(1)
            })
            .collect()
    }

    /// After a check: `measured` holds the name, the text length and the
    /// measured cost of every file the check type checked. A file whose
    /// entry still matches keeps it; when nothing changes there is nothing to
    /// record.
    pub(crate) fn record(&self, measured: &[(String, usize, u64)]) {
        let mut rows = measured
            .iter()
            .map(|(name, length, cost)| {
                (
                    name.as_str(),
                    *length as u64,
                    self.cost(name, *length).unwrap_or(*cost),
                )
            })
            .collect::<Vec<_>>();
        rows.sort_unstable();
        rows.dedup_by(|next, kept| next.0 == kept.0);
        let unchanged = rows.len() == self.entries.len()
            && rows
                .iter()
                .all(|&(name, length, cost)| self.entries.get(name) == Some(&(cost, length)));
        if unchanged {
            return;
        }
        let mut text = String::with_capacity(rows.len() * 64);
        text.push_str(HEADER);
        text.push('\n');
        for (name, length, cost) in rows {
            let _ = writeln!(text, "{cost}\t{length}\t{name}");
        }
        *self
            .update
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_cost_holds_while_the_text_keeps_its_length() {
        let profile = LaneProfile::parse("900\t10\t/a.ts\n300\t20\t/b.ts\n");
        assert_eq!(profile.cost("/a.ts", 10), Some(900));
        assert_eq!(profile.cost("/a.ts", 11), None);
        assert_eq!(profile.cost("/c.ts", 10), None);
        // /a.ts and /b.ts cost 1,200 for 400 of static part: an unrecorded
        // file weighs three times its own; a file that is not type checked
        // weighs one.
        let names = ["/a.ts", "/b.ts", "/c.ts", "/lib.d.ts"].map(str::to_owned);
        assert_eq!(
            profile.weights(&names, &[10, 20, 5, 7], &[100, 300, 50, 0]),
            [900, 300, 150, 1]
        );
        // Nothing recorded: the static parts themselves.
        assert_eq!(
            LaneProfile::parse("").weights(&names, &[10, 20, 5, 7], &[100, 300, 50, 0]),
            [100, 300, 50, 1]
        );
    }

    #[test]
    fn a_check_records_what_the_record_does_not_hold() {
        // A first check: everything is new, the rows are sorted by name.
        let empty = LaneProfile::parse("");
        assert!(empty.is_empty());
        empty.record(&[("/b.ts".to_owned(), 20, 300), ("/a.ts".to_owned(), 10, 900)]);
        let text = empty.take_update().expect("a first record");
        assert_eq!(text, format!("{HEADER}\n900\t10\t/a.ts\n300\t20\t/b.ts\n"));
        assert_eq!(empty.take_update(), None);

        // The same Program again, with other measured costs (another
        // dealing): the record holds, nothing is to be written.
        let recorded = LaneProfile::parse(&text);
        recorded.record(&[("/a.ts".to_owned(), 10, 700), ("/b.ts".to_owned(), 20, 350)]);
        assert_eq!(recorded.take_update(), None);

        // /b.ts changed its length and /c.ts is new: both are recorded as
        // measured, /a.ts keeps its entry; a file that left the Program
        // leaves the record.
        recorded.record(&[
            ("/a.ts".to_owned(), 10, 700),
            ("/b.ts".to_owned(), 21, 350),
            ("/c.ts".to_owned(), 5, 40),
        ]);
        assert_eq!(
            recorded.take_update().as_deref(),
            Some(format!("{HEADER}\n900\t10\t/a.ts\n350\t21\t/b.ts\n40\t5\t/c.ts\n").as_str())
        );
        recorded.record(&[("/a.ts".to_owned(), 10, 700)]);
        assert_eq!(
            recorded.take_update().as_deref(),
            Some(format!("{HEADER}\n900\t10\t/a.ts\n").as_str())
        );

        // A damaged text loses only the lines that are no entries.
        let damaged = LaneProfile::parse("900\t10\t/a.ts\n30");
        assert_eq!(damaged.cost("/a.ts", 10), Some(900));
        assert_eq!(damaged.cost("/b.ts", 20), None);
    }
}
