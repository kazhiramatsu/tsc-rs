//! Explicit checker-count budget and the deterministic file partition used by
//! sharded checking.
//!
//! tsrs-native: tsc has one checker; tsgo's `--checkers` (default 4,
//! independent of the CPU count) and `--singleThreaded` are the closest
//! controls. The budget is separate from the parse/bind
//! [`tsc_program::WorkerBudget`]: a shard is a complete checker state with its
//! own type-id domain over the shared immutable program snapshot, so the
//! number of shards decides duplicated per-checker work as well as
//! parallelism, and it is chosen explicitly, never from
//! `available_parallelism`. The API default is [`CheckerBudget::serial`], the
//! exact single-checker behaviour.

use std::num::NonZeroUsize;

/// The most checker shards one budget ever uses, regardless of the machine.
///
/// A provisional resource cap (tsgo caps at min(files, 256)): every shard
/// duplicates the checker's initialization and any shared dependency it
/// resolves, and the partition below is a load-balancing baseline that has not
/// been measured beyond this width. Revisit from the W measurements.
pub const MAX_CHECKERS: usize = 8;

/// The number of checker states (including the calling thread's) one program
/// check may construct.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckerBudget {
    checkers: NonZeroUsize,
    /// Leak every checker state instead of dropping it (the CLI's one-shot
    /// process exits right after publishing; tearing down the links tables,
    /// type tables and transient symbols was ~5 % of its sampled ticks).
    leak_states: bool,
}

/// The automatic budget's cap: tsgo's default checker count. More shards
/// currently cost more duplicated per-checker work than they save on this
/// machine (W measurements: 8 checkers were slower than 4 on scale256).
const AUTOMATIC_CHECKERS_CAP: usize = 4;

impl Default for CheckerBudget {
    /// The API default: one checker.
    fn default() -> Self {
        Self::serial()
    }
}

impl CheckerBudget {
    /// Exactly one checker: the sequential driver, which is the exact
    /// reference behaviour and the reproducible control.
    pub const fn serial() -> Self {
        Self {
            checkers: NonZeroUsize::MIN,
            leak_states: false,
        }
    }

    /// An explicit checker count, clamped to [`MAX_CHECKERS`]. The effective
    /// count is further limited to the number of program files at partition
    /// time.
    pub const fn new(checkers: NonZeroUsize) -> Self {
        let clamped = if checkers.get() > MAX_CHECKERS {
            MAX_CHECKERS
        } else {
            checkers.get()
        };
        Self {
            checkers: match NonZeroUsize::new(clamped) {
                Some(value) => value,
                None => NonZeroUsize::MIN,
            },
            leak_states: false,
        }
    }

    /// Whether checker states are leaked at the end of the check instead of
    /// dropped; only a process that exits right afterwards should set this.
    pub const fn with_leaked_states(mut self, leak: bool) -> Self {
        self.leak_states = leak;
        self
    }

    pub const fn leaks_states(self) -> bool {
        self.leak_states
    }

    /// The CLI default: one checker per available hardware thread, capped
    /// at tsgo's default of four (and, at partition time, by the file
    /// count). `TSRS_CHECKERS` pins another count.
    pub fn automatic() -> Self {
        let available = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
        Self::new(
            NonZeroUsize::new(available.clamp(1, AUTOMATIC_CHECKERS_CAP))
                .expect("at least one checker"),
        )
    }

    pub const fn checkers(self) -> usize {
        self.checkers.get()
    }

    /// Whether more than one checker state may be constructed.
    pub const fn is_sharded(self) -> bool {
        self.checkers.get() > 1
    }
}

/// Assign program files (by index) to at most `shards` checkers.
///
/// Deterministic least-load baseline: files are visited in program order and
/// each goes to the shard with the smallest accumulated weight, lowest shard
/// index on ties; every shard's list therefore stays in program order. Empty
/// shards are dropped, so the result has `min(shards, files)` entries when
/// `files` is non-empty. This is a simpler baseline than tsgo's weighted
/// graph partition (import affinity + load cap); it is measured against
/// that design in the W follow-up, not assumed equivalent.
pub(crate) fn partition_files(weights: &[usize], shards: usize) -> Vec<Vec<usize>> {
    let shards = shards.clamp(1, weights.len().max(1));
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut load = vec![0usize; shards];
    for (file, &weight) in weights.iter().enumerate() {
        let target = (0..shards)
            .min_by_key(|&shard| (load[shard], shard))
            .expect("at least one shard");
        assignment[target].push(file);
        load[target] += weight.max(1);
    }
    assignment.retain(|files| !files.is_empty());
    assignment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_are_explicit_and_bounded() {
        assert_eq!(CheckerBudget::default(), CheckerBudget::serial());
        assert_eq!(CheckerBudget::serial().checkers(), 1);
        assert!(!CheckerBudget::serial().is_sharded());
        let four = CheckerBudget::new(NonZeroUsize::new(4).unwrap());
        assert_eq!(four.checkers(), 4);
        assert!(four.is_sharded());
        assert_eq!(
            CheckerBudget::new(NonZeroUsize::new(1_000).unwrap()).checkers(),
            MAX_CHECKERS
        );
    }

    #[test]
    fn partition_covers_every_file_once_in_program_order() {
        let weights: Vec<usize> = (0..37).map(|file| (file * 7919) % 101 + 1).collect();
        for shards in [1, 2, 3, 4, 8, 64] {
            let assignment = partition_files(&weights, shards);
            assert_eq!(
                assignment.len(),
                shards.min(weights.len()),
                "shards={shards}"
            );
            let mut seen = vec![0usize; weights.len()];
            for files in &assignment {
                assert!(!files.is_empty());
                assert!(
                    files.windows(2).all(|pair| pair[0] < pair[1]),
                    "program order"
                );
                for &file in files {
                    seen[file] += 1;
                }
            }
            assert!(seen.iter().all(|&count| count == 1), "shards={shards}");
            assert_eq!(
                assignment,
                partition_files(&weights, shards),
                "deterministic"
            );
        }
    }

    #[test]
    fn partition_balances_by_weight_and_breaks_ties_by_shard_index() {
        // One heavy file, then light ones: the light files go to the other shard.
        assert_eq!(
            partition_files(&[100, 1, 1, 1], 2),
            vec![vec![0], vec![1, 2, 3]]
        );
        // Equal weights alternate, lowest index first.
        assert_eq!(
            partition_files(&[1, 1, 1, 1], 2),
            vec![vec![0, 2], vec![1, 3]]
        );
        // Zero weights still count as one unit so every file is scheduled.
        assert_eq!(
            partition_files(&[0, 0, 0], 3),
            vec![vec![0], vec![1], vec![2]]
        );
        assert!(partition_files(&[], 4).is_empty());
    }
}
