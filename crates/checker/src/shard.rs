//! Explicit checker-count budget and the file queue (shared Program-order
//! pulling or the deterministic node-count partition) used by sharded
//! checking.
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
use std::sync::atomic::{AtomicUsize, Ordering};

/// The most checker shards one budget ever uses, regardless of the machine.
///
/// A provisional resource cap (tsgo caps at min(files, 256)): every shard
/// duplicates the checker's initialization and any shared dependency it
/// resolves, and the queue below has not been measured beyond this width. Revisit from the W measurements.
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

/// The automatic checker count is capped at eight: on a ten-core machine
/// (four performance, six efficiency cores) a 256-file program checks about
/// 6% faster with eight shards than with tsgo's default of four, and no
/// faster with ten, while every extra shard repeats the lazy library-type
/// work (CPU +19% at eight). tsgo's default stays four.
const AUTOMATIC_CHECKERS_CAP: usize = 8;

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
    /// at `AUTOMATIC_CHECKERS_CAP` (and, at partition time, by the file
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
/// Deterministic least-load, heaviest first: files are visited from the
/// heaviest down (program order between equal weights) and each goes to the
/// shard with the smallest accumulated weight, lowest shard index on ties,
/// so one source heavier than the rest of a shard's share ends up alone
/// instead of sharing its checker with the light files scheduled before it
/// in program order. Every shard's list is then sorted back into program
/// order. Empty shards are dropped, so the result has `min(shards, files)`
/// entries when `files` is non-empty. This is a simpler baseline than tsgo's
/// weighted graph partition (import affinity + load cap); it is measured
/// against that design in the W follow-up, not assumed equivalent.
pub(crate) fn partition_files(weights: &[usize], shards: usize) -> Vec<Vec<usize>> {
    let shards = shards.clamp(1, weights.len().max(1));
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by_key(|&file| (std::cmp::Reverse(weights[file].max(1)), file));
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut load = vec![0usize; shards];
    for file in order {
        let target = (0..shards)
            .min_by_key(|&shard| (load[shard], shard))
            .expect("at least one shard");
        assignment[target].push(file);
        load[target] += weights[file].max(1);
    }
    for files in &mut assignment {
        files.sort_unstable();
    }
    assignment.retain(|files| !files.is_empty());
    assignment
}

/// The work queue the checker shards pull their files from, in one of two
/// modes.
///
/// Shared: every shard pulls the next fixture from one Program-order cursor
/// until none is left, then (when the driver completes the library prefix)
/// the next library file. Each pull is one atomic increment, so a shard
/// still checks its files in increasing Program order while the split
/// between shards follows the measured check times: a shard on a slower
/// core simply takes fewer files instead of finishing last (the node-count
/// partition's eight equal shares finished up to 2x apart on a mixed
/// performance/efficiency-core machine). Which files a shard checks is then
/// not reproducible run to run; the merged result is, because every shard
/// checks each file exactly as the serial driver would and the order guard
/// replays any order-consuming shard-local operation serially.
///
/// Partitioned: each shard pulls only from its own share of the
/// deterministic node-count partition ([`partition_files`]). The driver
/// chooses this when the phases after the check — the declaration
/// diagnostics gate and the emit, which run each file on the checker that
/// checked it — would outweigh the check (declaration emit), or when there
/// are too few fixtures per shard for the pull to balance anything while
/// the partition still spreads the library pass from the start.
#[derive(Debug)]
pub(crate) struct ShardFileQueue {
    lib_count: usize,
    file_count: usize,
    lanes: Lanes,
}

#[derive(Debug)]
enum Lanes {
    Shared {
        shards: usize,
        /// Node count each shard sizes its type tables for: the mean share,
        /// or the largest single file when that is bigger (a file is never
        /// split).
        reserved_nodes: usize,
        next_fixture: AtomicUsize,
        next_library: AtomicUsize,
    },
    Partitioned(Vec<Lane>),
}

#[derive(Debug)]
struct Lane {
    fixtures: Vec<usize>,
    libraries: Vec<usize>,
    weight: usize,
    next_fixture: AtomicUsize,
    next_library: AtomicUsize,
}

impl ShardFileQueue {
    /// One shared Program-order cursor per pass over the files whose node
    /// counts are `weights` (index = Program file; `0..lib_count` are the
    /// libraries), served to at most `shards` shards.
    pub(crate) fn shared(lib_count: usize, weights: &[usize], shards: usize) -> Self {
        let file_count = weights.len();
        debug_assert!(lib_count <= file_count);
        let shards = shards.clamp(1, file_count.max(1));
        let total: usize = weights.iter().sum();
        let largest = weights.iter().copied().max().unwrap_or(0);
        Self {
            lib_count,
            file_count,
            lanes: Lanes::Shared {
                shards,
                reserved_nodes: (total / shards).max(largest),
                next_fixture: AtomicUsize::new(lib_count),
                next_library: AtomicUsize::new(0),
            },
        }
    }

    /// One lane per non-empty share of the deterministic node-count
    /// partition of `weights` into at most `shards` shares.
    pub(crate) fn partitioned(lib_count: usize, weights: &[usize], shards: usize) -> Self {
        debug_assert!(lib_count <= weights.len());
        let lanes = partition_files(weights, shards)
            .into_iter()
            .map(|files| Lane {
                weight: files.iter().map(|&file| weights[file]).sum(),
                fixtures: files
                    .iter()
                    .copied()
                    .filter(|&file| file >= lib_count)
                    .collect(),
                libraries: files
                    .iter()
                    .copied()
                    .filter(|&file| file < lib_count)
                    .collect(),
                next_fixture: AtomicUsize::new(0),
                next_library: AtomicUsize::new(0),
            })
            .collect();
        Self {
            lib_count,
            file_count: weights.len(),
            lanes: Lanes::Partitioned(lanes),
        }
    }

    /// The number of shards this queue serves (at least one).
    pub(crate) fn shard_count(&self) -> usize {
        match &self.lanes {
            Lanes::Shared { shards, .. } => *shards,
            Lanes::Partitioned(lanes) => lanes.len().max(1),
        }
    }

    /// The node count `shard` should size its type tables for.
    pub(crate) fn reserved_nodes(&self, shard: usize) -> usize {
        match &self.lanes {
            Lanes::Shared { reserved_nodes, .. } => *reserved_nodes,
            Lanes::Partitioned(lanes) => lanes.get(shard).map_or(0, |lane| lane.weight),
        }
    }

    /// The next unchecked fixture for `shard`, in Program order, if any.
    pub(crate) fn next_fixture(&self, shard: usize) -> Option<usize> {
        match &self.lanes {
            Lanes::Shared { next_fixture, .. } => {
                let file = next_fixture.fetch_add(1, Ordering::Relaxed);
                (file < self.file_count).then_some(file)
            }
            Lanes::Partitioned(lanes) => {
                let lane = lanes.get(shard)?;
                let index = lane.next_fixture.fetch_add(1, Ordering::Relaxed);
                lane.fixtures.get(index).copied()
            }
        }
    }

    /// The next unchecked library file for `shard`, in Program order, if any.
    pub(crate) fn next_library(&self, shard: usize) -> Option<usize> {
        match &self.lanes {
            Lanes::Shared { next_library, .. } => {
                let file = next_library.fetch_add(1, Ordering::Relaxed);
                (file < self.lib_count).then_some(file)
            }
            Lanes::Partitioned(lanes) => {
                let lane = lanes.get(shard)?;
                let index = lane.next_library.fetch_add(1, Ordering::Relaxed);
                lane.libraries.get(index).copied()
            }
        }
    }
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

    #[test]
    fn shared_queue_hands_out_every_file_once_in_program_order() {
        let weights = [4, 1, 9, 2, 2, 2, 2, 2, 2, 2];
        let queue = ShardFileQueue::shared(3, &weights, 8);
        assert_eq!(queue.shard_count(), 8);
        // The mean share (28 / 8 = 3) is below the largest file.
        assert_eq!(queue.reserved_nodes(5), 9);
        let fixtures: Vec<usize> = std::iter::from_fn(|| queue.next_fixture(0)).collect();
        assert_eq!(fixtures, vec![3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(queue.next_fixture(1), None);
        let libraries: Vec<usize> = std::iter::from_fn(|| queue.next_library(7)).collect();
        assert_eq!(libraries, vec![0, 1, 2]);
        assert_eq!(queue.next_library(0), None);
        let empty = ShardFileQueue::shared(0, &[], 8);
        assert_eq!(empty.shard_count(), 1);
        assert_eq!(empty.next_fixture(0), None);
        assert_eq!(empty.next_library(0), None);
        // Sixteen equal files over four shards: the mean share wins.
        assert_eq!(ShardFileQueue::shared(0, &[2; 16], 4).reserved_nodes(0), 8);
    }

    #[test]
    fn shared_queue_splits_files_between_concurrent_shards_without_overlap() {
        let queue = ShardFileQueue::shared(4, &[1; 64], 4);
        let pulls: Vec<Vec<usize>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|shard| {
                    let queue = &queue;
                    scope.spawn(move || {
                        let mut files: Vec<usize> =
                            std::iter::from_fn(|| queue.next_fixture(shard)).collect();
                        files.extend(std::iter::from_fn(|| queue.next_library(shard)));
                        files
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let mut seen = vec![0usize; 64];
        for files in &pulls {
            // Fixtures and then libraries each arrive in increasing order.
            let split = files
                .iter()
                .position(|&file| file < 4)
                .unwrap_or(files.len());
            assert!(files[..split].windows(2).all(|pair| pair[0] < pair[1]));
            assert!(files[split..].windows(2).all(|pair| pair[0] < pair[1]));
            for &file in files {
                seen[file] += 1;
            }
        }
        assert!(seen.iter().all(|&count| count == 1));
    }

    #[test]
    fn partitioned_queue_serves_each_shard_its_own_share_in_program_order() {
        // File 0 is the (heavy) library; the fixtures are light.
        let weights = [5, 1, 1, 1, 1, 1, 1];
        let queue = ShardFileQueue::partitioned(1, &weights, 2);
        let expected = partition_files(&weights, 2);
        assert_eq!(queue.shard_count(), expected.len());
        for (shard, share) in expected.iter().enumerate() {
            let fixtures: Vec<usize> = std::iter::from_fn(|| queue.next_fixture(shard)).collect();
            let libraries: Vec<usize> = std::iter::from_fn(|| queue.next_library(shard)).collect();
            assert!(fixtures.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(fixtures.iter().all(|&file| file >= 1));
            assert!(libraries.iter().all(|&file| file < 1));
            let mut files = fixtures;
            files.extend(libraries);
            files.sort_unstable();
            assert_eq!(&files, share, "shard {shard}");
            assert_eq!(
                queue.reserved_nodes(shard),
                share.iter().map(|&file| weights[file]).sum::<usize>()
            );
            assert_eq!(queue.next_fixture(shard), None);
            assert_eq!(queue.next_library(shard), None);
        }
        // A shard index beyond the partition serves nothing.
        assert_eq!(queue.next_fixture(expected.len()), None);
        assert_eq!(queue.reserved_nodes(expected.len()), 0);
        let empty = ShardFileQueue::partitioned(0, &[], 4);
        assert_eq!(empty.shard_count(), 1);
        assert_eq!(empty.next_fixture(0), None);
    }
}
