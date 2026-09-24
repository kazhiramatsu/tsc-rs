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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The most checker shards one budget ever uses, regardless of the machine.
///
/// A provisional resource cap (tsgo caps at min(files, 256)): every shard
/// duplicates the checker's initialization and any shared dependency it
/// resolves. Sixteen admits every hardware thread of the machines measured
/// so far (a pinned `TSRS_CHECKERS=10` was silently eight before); the
/// automatic budget stays at [`AUTOMATIC_CHECKERS_CAP`]: on VS Code `src`
/// ten shards cost 7% more CPU than eight for no reliable wall gain.
pub const MAX_CHECKERS: usize = 16;

/// The number of checker states (including the calling thread's) one program
/// check may construct.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckerBudget {
    checkers: NonZeroUsize,
    /// The count came from [`CheckerBudget::automatic`] rather than an
    /// explicit request, so a static partition may take more shards.
    automatic: bool,
    /// Leak every checker state instead of dropping it (the CLI's one-shot
    /// process exits right after publishing; tearing down the links tables,
    /// type tables and transient symbols was ~5 % of its sampled ticks).
    leak_states: bool,
    /// Discard an order-consuming sharded result for a serial replay (the
    /// exact mode); see [`CheckerBudget::with_order_replay`].
    order_replay: bool,
}

/// The automatic checker count is capped at eight: on a ten-core machine
/// (four performance, six efficiency cores) a 256-file program checks about
/// 6% faster with eight shards than with tsgo's default of four, and no
/// faster with ten, while every extra shard repeats the lazy library-type
/// work (CPU +19% at eight). tsgo's default stays four.
const AUTOMATIC_CHECKERS_CAP: usize = 8;

/// tsrs-native diagnostic control: `TSRS_ORDER_REPLAY=1` requests the exact
/// mode of [`CheckerBudget::with_order_replay`] from the environment.
pub fn order_replay_requested() -> bool {
    std::env::var_os("TSRS_ORDER_REPLAY").is_some_and(|v| v == "1")
}

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
            automatic: false,
            leak_states: false,
            order_replay: false,
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
            automatic: false,
            leak_states: false,
            order_replay: false,
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

    /// Whether a shard's order-consuming operation discards the sharded
    /// result for a serial replay (the exact mode). By default the order
    /// guard is telemetry only: every real program consumes shard-local
    /// type order in its first files (union subtype reduction, common
    /// supertypes, union signatures), so the replay made the parallel check
    /// a wasted prologue to a serial one. The accepted divergences are those
    /// tsgo accepts as well: orderings that follow type ids (union
    /// constituents in displayed types, declaration output order), and the
    /// order-dependent inference results such constructions produce; a run
    /// is deterministic because the partition is. One checker remains the
    /// exact serial mode.
    pub const fn with_order_replay(mut self, replay: bool) -> Self {
        self.order_replay = replay;
        self
    }

    pub const fn order_replay(self) -> bool {
        self.order_replay
    }

    /// The CLI default: one checker per available hardware thread, capped
    /// at `AUTOMATIC_CHECKERS_CAP` (and, at partition time, by the file
    /// count). `TSRS_CHECKERS` pins another count.
    pub fn automatic() -> Self {
        let available = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let mut budget = Self::new(
            NonZeroUsize::new(available.clamp(1, AUTOMATIC_CHECKERS_CAP))
                .expect("at least one checker"),
        );
        budget.automatic = true;
        budget
    }

    pub const fn checkers(self) -> usize {
        self.checkers.get()
    }

    /// Whether the count came from [`CheckerBudget::automatic`] (a driver
    /// may then lower it for the program at hand) rather than an explicit
    /// request, which is kept as made.
    pub const fn is_automatic(self) -> bool {
        self.automatic
    }

    /// The checker count for a partition nothing rebalances later (the
    /// static split of a declaration-emitting run): one and a half times the
    /// automatic count, capped at [`MAX_CHECKERS`]. Smaller shares leave a
    /// shorter tail behind the heaviest share: on a ten-core machine the
    /// hono, playwright, zod and Next.js declaration runs were 4–13 % faster
    /// with twelve shards than with eight, ten gained nothing and sixteen
    /// lost. An explicit count (`TSRS_CHECKERS`) is kept as requested.
    pub const fn checkers_for_static_partition(self) -> usize {
        if self.automatic {
            let scaled = self.checkers.get() * 3 / 2;
            if scaled > MAX_CHECKERS {
                MAX_CHECKERS
            } else {
                scaled
            }
        } else {
            self.checkers.get()
        }
    }

    /// Whether more than one checker state may be constructed.
    pub const fn is_sharded(self) -> bool {
        self.checkers.get() > 1
    }
}

/// The most shards worth running for a program whose heaviest checked file
/// weighs `heaviest` of `total` and dominates it (a quarter of the checked
/// nodes or more): the check cannot finish before that file's check, so
/// shards beyond `total / heaviest` (rounded up) shorten no schedule; they
/// only repeat the lazy library-type work and contend with the critical
/// shard for cores and memory. The TypeScript compiler's `checker.ts` is
/// about a third of its program: four shards check it in 351 ms where
/// eight take 380 ms (tsgo: 355 ms), and emit it in 648 ms where eight
/// take 682 ms. Below that share the node count of the heaviest file says
/// little about its check time (hono's `types.ts` is a seventh of the
/// nodes and a cheap check; its static declaration split lost 25 % of its
/// time to a cap of seven), so no cap applies.
pub(crate) fn dominant_file_shard_cap(total: usize, heaviest: usize) -> usize {
    if heaviest == 0 || heaviest * 4 < total {
        return usize::MAX;
    }
    total.div_ceil(heaviest).max(1)
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
    order.sort_by_key(|&file| (std::cmp::Reverse(cost_weight(weights[file])), file));
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut load = vec![0u64; shards];
    for file in order {
        let target = (0..shards)
            .min_by_key(|&shard| (load[shard], shard))
            .expect("at least one shard");
        assignment[target].push(file);
        load[target] += cost_weight(weights[file]);
    }
    for files in &mut assignment {
        files.sort_unstable();
    }
    assignment.retain(|files| !files.is_empty());
    assignment
}

/// The partition's cost estimate for a source of `nodes` syntax nodes: the
/// node count scaled by its logarithm. Checking cost per node grows with the
/// source (the largest sources carry the deepest types), so the plain node
/// count left the shard holding the largest sources finishing last; on the
/// Next.js package the heaviest shard's excess over the mean fell with this
/// scaling.
fn cost_weight(nodes: usize) -> u64 {
    let nodes = nodes.max(1) as u64;
    nodes * (u64::from(nodes.ilog2()) + 1)
}

/// How the deterministic partition deals files to shards. The default is
/// [`partition_files_by_directory`] with a load slack of
/// [`DEFAULT_DIRECTORY_SLACK_PERCENT`]; `TSRS_SHARD_PARTITION` selects
/// another for measurement: `leastload` ([`partition_files`]), `contiguous`
/// ([`partition_files_contiguous`]), `blocks` or `blocks:N`
/// ([`partition_files_blocks`], N blocks per shard, default 4), `directory:P`
/// (slack of P percent of a share).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartitionMode {
    LeastLoad,
    Contiguous,
    Blocks(usize),
    Directory(u64),
}

/// The directory-preferring partition's load slack, in percent of an equal
/// share: measured against the plain least-load spread with eight checkers,
/// 10% took VS Code `src` from 5.26 to 5.05 s (5% to 4.95 s), Playwright
/// from 635 to 549 ms (5%: 572 ms) and the Next.js root from 788 to 757 ms
/// (5%: 801 ms), TypeScript `src/compiler` unchanged; CPU fell 5–30%.
const DEFAULT_DIRECTORY_SLACK_PERCENT: u64 = 10;

pub(crate) fn partition_mode_requested() -> PartitionMode {
    static MODE: std::sync::OnceLock<PartitionMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        let value = std::env::var("TSRS_SHARD_PARTITION").unwrap_or_default();
        let argument = |rest: &str, default: u64| {
            rest.strip_prefix(':')
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(default)
        };
        if value == "leastload" {
            PartitionMode::LeastLoad
        } else if value == "contiguous" {
            PartitionMode::Contiguous
        } else if let Some(rest) = value.strip_prefix("blocks") {
            PartitionMode::Blocks(argument(rest, 4).max(1) as usize)
        } else if let Some(rest) = value.strip_prefix("directory") {
            PartitionMode::Directory(argument(rest, DEFAULT_DIRECTORY_SLACK_PERCENT))
        } else {
            PartitionMode::Directory(DEFAULT_DIRECTORY_SLACK_PERCENT)
        }
    })
}

/// Assign program files (by index) to at most `shards` checkers by least
/// load with a directory preference: among the shards whose load is within
/// `slack_percent` of a share above the lightest, the one already holding
/// the most files of the file's directory takes it (ties: lighter load,
/// then lower index). Files of one directory import each other and the
/// same modules, so a shard resolves fewer foreign declarations than under
/// the plain least-load spread while every load stays within the slack of
/// the lightest. A directory of two equal shares of the files or more gets
/// no preference (nothing to contain; the slack would only skew the load).
/// Files are dealt heaviest first like [`partition_files`]. Deterministic.
#[cfg(test)]
pub(crate) fn partition_files_by_directory(
    weights: &[usize],
    directories: &[u32],
    shards: usize,
    slack_percent: u64,
) -> Vec<Vec<usize>> {
    partition_files_by_directory_with_cost(
        weights,
        &[],
        directories,
        shards,
        slack_percent,
        CostModel::NodesLog,
    )
}

/// The static check-cost estimate of a file for the directory-preferring
/// partition. The default is the plain node count: measured at one binary
/// with eight checkers, it took VS Code `src` from 5.03 to 4.84 s (the
/// slowest shard 4.10 → 3.61 s), Playwright from 558 to 486 ms and the
/// Next.js root from 790 to 774 ms against the node count scaled by its
/// logarithm (`packages/next` 930 → 969 ms), and a symbol-count term did no
/// better on any of them. `TSRS_SHARD_COST` selects another for
/// measurement: `nlog` ([`cost_weight`]), or `symbols:K` (node count plus K
/// times the binder's symbol count).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CostModel {
    NodesLog,
    Nodes,
    NodesSymbols(u64),
}

pub(crate) fn cost_model_requested() -> CostModel {
    static MODEL: std::sync::OnceLock<CostModel> = std::sync::OnceLock::new();
    *MODEL.get_or_init(|| {
        let value = std::env::var("TSRS_SHARD_COST").unwrap_or_default();
        if value == "nlog" {
            CostModel::NodesLog
        } else if let Some(rest) = value.strip_prefix("symbols") {
            CostModel::NodesSymbols(
                rest.strip_prefix(':')
                    .and_then(|k| k.parse::<u64>().ok())
                    .unwrap_or(8),
            )
        } else {
            CostModel::Nodes
        }
    })
}

fn cost_of(model: CostModel, nodes: usize, symbols: usize) -> u64 {
    match model {
        CostModel::NodesLog => cost_weight(nodes),
        CostModel::Nodes => nodes.max(1) as u64,
        CostModel::NodesSymbols(k) => nodes.max(1) as u64 + k * symbols as u64,
    }
}

/// [`partition_files_by_directory`] with the file cost chosen by `model`
/// (`symbols`, index = Program file, may be empty).
pub(crate) fn partition_files_by_directory_with_cost(
    weights: &[usize],
    symbols: &[usize],
    directories: &[u32],
    shards: usize,
    slack_percent: u64,
    model: CostModel,
) -> Vec<Vec<usize>> {
    let shards = shards.clamp(1, weights.len().max(1));
    let costs: Vec<u64> = weights
        .iter()
        .enumerate()
        .map(|(file, &nodes)| cost_of(model, nodes, symbols.get(file).copied().unwrap_or(0)))
        .collect();
    let total: u64 = costs.iter().sum();
    let slack = total / shards as u64 * slack_percent / 100;
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by_key(|&file| (std::cmp::Reverse(costs[file]), file));
    // A directory holding two equal shares of the files or more gains
    // nothing from the preference (its files land everywhere anyway) and
    // would pay the slack as imbalance: TypeScript's src/compiler keeps
    // its 248 files in one directory. Such files take the lightest shard.
    let mut directory_sizes: rustc_hash::FxHashMap<u32, usize> = Default::default();
    for &directory in directories {
        *directory_sizes.entry(directory).or_insert(0) += 1;
    }
    let preferred = |directory: u32| {
        directory_sizes
            .get(&directory)
            .is_some_and(|&size| size * shards < weights.len() * 2)
    };
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut load = vec![0u64; shards];
    let mut affinity: Vec<rustc_hash::FxHashMap<u32, u32>> = vec![Default::default(); shards];
    for file in order {
        let directory = directories.get(file).copied().unwrap_or(u32::MAX);
        let lightest = load.iter().copied().min().unwrap_or(0);
        let limit = if preferred(directory) {
            lightest.saturating_add(slack)
        } else {
            lightest
        };
        let target = (0..shards)
            .filter(|&shard| load[shard] <= limit)
            .max_by_key(|&shard| {
                (
                    affinity[shard].get(&directory).copied().unwrap_or(0),
                    std::cmp::Reverse(load[shard]),
                    std::cmp::Reverse(shard),
                )
            })
            .expect("the lightest shard is within the limit");
        assignment[target].push(file);
        load[target] += costs[file];
        *affinity[target].entry(directory).or_insert(0) += 1;
    }
    for files in &mut assignment {
        files.sort_unstable();
    }
    assignment.retain(|files| !files.is_empty());
    assignment
}

/// Split program order into `blocks` contiguous ranges of about equal cost
/// (the closing rule of [`partition_files_contiguous`]).
fn contiguous_ranges(costs: &[i64], blocks: usize) -> Vec<Vec<usize>> {
    let blocks = blocks.clamp(1, costs.len().max(1));
    let mut remaining_total: i64 = costs.iter().sum();
    let mut ranges: Vec<Vec<usize>> = Vec::with_capacity(blocks);
    let mut current: Vec<usize> = Vec::new();
    let mut current_cost: i64 = 0;
    for (file, &cost) in costs.iter().enumerate() {
        let remaining_blocks = (blocks - ranges.len()) as i64;
        if remaining_blocks > 1 && !current.is_empty() {
            let share = remaining_total / remaining_blocks;
            let overshoot = current_cost + cost - share;
            let shortfall = share - current_cost;
            if overshoot > 0 && overshoot > shortfall {
                ranges.push(std::mem::take(&mut current));
                current_cost = 0;
            }
        }
        current.push(file);
        current_cost += cost;
        remaining_total -= cost;
    }
    if !current.is_empty() {
        ranges.push(current);
    }
    ranges
}

/// Assign program files to at most `shards` checkers as contiguous
/// Program-order blocks, `blocks_per_shard` of them per shard, dealt to the
/// shards heaviest first by least load like [`partition_files`] deals files.
///
/// A block's files share their imports (Program order lists a file's
/// imports before it), so a shard resolves the declarations of a few regions
/// of the import graph instead of a scattered sample of the whole program,
/// while dealing several blocks per shard evens out the check cost that the
/// node count does not predict (one whole-program range per shard checked
/// three times slower than another on VS Code). Deterministic.
pub(crate) fn partition_files_blocks(
    weights: &[usize],
    shards: usize,
    blocks_per_shard: usize,
) -> Vec<Vec<usize>> {
    let shards = shards.clamp(1, weights.len().max(1));
    let costs: Vec<i64> = weights
        .iter()
        .map(|&nodes| i64::try_from(cost_weight(nodes)).unwrap_or(i64::MAX / 4))
        .collect();
    let blocks = contiguous_ranges(&costs, shards * blocks_per_shard.max(1));
    let block_cost = |block: &Vec<usize>| -> i64 { block.iter().map(|&file| costs[file]).sum() };
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&block| (std::cmp::Reverse(block_cost(&blocks[block])), block));
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut load = vec![0i64; shards];
    for block in order {
        let target = (0..shards)
            .min_by_key(|&shard| (load[shard], shard))
            .expect("at least one shard");
        load[target] += block_cost(&blocks[block]);
        assignment[target].extend_from_slice(&blocks[block]);
    }
    for files in &mut assignment {
        files.sort_unstable();
    }
    assignment.retain(|files| !files.is_empty());
    assignment
}

/// Assign program files (by index) to at most `shards` checkers as
/// contiguous Program-order ranges of about equal cost.
///
/// Program order lists a file's imports before it, so the files of one
/// range share their imports: each shard then resolves the declarations of
/// one region of the import graph, where the least-load partition's
/// heaviest-first spread has every shard resolve most of the program. A
/// range closes when the next file would overshoot the remaining equal
/// share by more than it would fall short; the last range takes the rest,
/// so a file heavier than a share is a range of its own. Deterministic like
/// [`partition_files`].
pub(crate) fn partition_files_contiguous(weights: &[usize], shards: usize) -> Vec<Vec<usize>> {
    let shards = shards.clamp(1, weights.len().max(1));
    let costs: Vec<i64> = weights
        .iter()
        .map(|&nodes| i64::try_from(cost_weight(nodes)).unwrap_or(i64::MAX / 4))
        .collect();
    let mut remaining_total: i64 = costs.iter().sum();
    let mut assignment: Vec<Vec<usize>> = Vec::with_capacity(shards);
    let mut current: Vec<usize> = Vec::new();
    let mut current_cost: i64 = 0;
    for (file, &cost) in costs.iter().enumerate() {
        let remaining_shards = (shards - assignment.len()) as i64;
        if remaining_shards > 1 && !current.is_empty() {
            let share = remaining_total / remaining_shards;
            let overshoot = current_cost + cost - share;
            let shortfall = share - current_cost;
            if overshoot > 0 && overshoot > shortfall {
                assignment.push(std::mem::take(&mut current));
                current_cost = 0;
            }
        }
        current.push(file);
        current_cost += cost;
        remaining_total -= cost;
    }
    if !current.is_empty() {
        assignment.push(current);
    }
    assignment
}

/// How the shards take their files. `TSRS_SHARD_QUEUE=shared` selects the
/// shared Program-order queue (see [`ShardFileQueue`]) pulling one file per
/// turn, `shared:K` pulling K consecutive files per turn, `partition` the
/// deterministic partition; unset, the driver picks by program size and
/// command (see [`SHARED_QUEUE_MIN_FIXTURES`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueueMode {
    Shared(usize),
    Partition,
}

/// Files per pull of the default shared queue: measured at one binary with
/// eight checkers on VS Code `src`, chunks of 8 / 32 / 128 files took the
/// check from 4.46 s (static partition) to 4.20 / 4.16 / 4.21 s, every
/// shard finishing within 3% of the others.
pub(crate) const DEFAULT_SHARED_CHUNK: usize = 32;

/// From this many fixtures a `--noEmit` check pulls from the shared queue
/// by default. Below it the static directory-preferring partition wins:
/// the balance the queue buys is small against the resolution locality it
/// loses (Playwright, 646 files: 478 → 513 ms; Next.js `packages/next`,
/// 1,543 files: 936 → 1,132 ms; the Next.js root, 3,427 files: 765 → 767
/// ms; VS Code `src`, 9,442 files: 4.46 → 4.16 s). The queue's split
/// between shards follows the measured check times, so which shard checks
/// a file varies run to run; the diagnostics of the projects measured were
/// byte-identical across runs.
pub(crate) const SHARED_QUEUE_MIN_FIXTURES: usize = 4096;

/// From this many fixtures an exhausted partition lane steals from the
/// others: zod (2,364 fixtures) checked in 723 ms with stealing against
/// 983 ms without at one binary, while a small program keeps its static
/// shares and therefore its run-to-run type order.
pub(crate) const STEAL_MIN_FIXTURES: usize = 256;

/// Whether an exhausted partition lane may steal from the others (default;
/// `TSRS_SHARD_STEAL=0` keeps the static shares at every size).
pub(crate) fn stealing_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TSRS_SHARD_STEAL")
            .ok()
            .is_none_or(|value| value != "0")
    })
}

pub(crate) fn queue_mode_requested() -> Option<QueueMode> {
    static MODE: std::sync::OnceLock<Option<QueueMode>> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        let value = std::env::var("TSRS_SHARD_QUEUE").unwrap_or_default();
        if value == "partition" {
            return Some(QueueMode::Partition);
        }
        let rest = value.strip_prefix("shared")?;
        Some(QueueMode::Shared(
            rest.strip_prefix(':')
                .and_then(|k| k.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1),
        ))
    })
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
/// performance/efficiency-core machine). A file heavier than an equal share
/// dominates whichever shard checks it, so it is pinned to a shard of its
/// own (heaviest first, at most `shards - 1` of them) and checked from the
/// start instead of waiting its turn in Program order behind that shard's
/// lighter pulls: a 1 MB source last in Program order, or the largest
/// library file, otherwise started 1-2 ms late. Which files a shard checks
/// is then not reproducible run to run; the merged result is, because every
/// shard checks each file exactly as the serial driver would and the order
/// guard replays any order-consuming shard-local operation serially.
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
    /// Partitioned lanes only: an exhausted lane steals from the others
    /// (see [`Self::steal_fixture`]).
    steal: bool,
    /// Set once any shard's order guard has recorded a reason: the driver
    /// will discard every shard result and replay serially, so the shards
    /// stop pulling files instead of finishing work that is thrown away.
    aborted: AtomicBool,
}

#[derive(Debug)]
enum Lanes {
    Shared {
        shards: usize,
        /// Shards `0..pinned.len()` each check one file heavier than an
        /// equal share; the shards after them pull from the cursors.
        pinned: Vec<Lane>,
        /// The pinned files, ascending, skipped by the cursors.
        pinned_files: Vec<usize>,
        /// Node count each pulling shard sizes its type tables for: the mean
        /// share of the unpinned files, or the largest of them when that is
        /// bigger (a file is never split).
        reserved_nodes: usize,
        next_fixture: AtomicUsize,
        next_library: AtomicUsize,
        /// Files taken from a cursor per pull: a run of consecutive Program
        /// files shares its imports (the include walk lists a directory's
        /// files together), so a chunk keeps the shard's resolution local
        /// while the pulls still balance the shards; one file per pull
        /// (the original queue) spreads every directory over every shard.
        chunk: usize,
        /// Each shard's current chunk of fixtures and of libraries as
        /// `(next, end)`; only its own shard touches a pair.
        local_fixture: Vec<(AtomicUsize, AtomicUsize)>,
        local_library: Vec<(AtomicUsize, AtomicUsize)>,
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
    #[cfg(test)]
    pub(crate) fn shared(lib_count: usize, weights: &[usize], shards: usize) -> Self {
        Self::shared_chunked(lib_count, weights, shards, 1)
    }

    /// [`shared`](Self::shared) pulling `chunk` consecutive Program files
    /// per turn at the cursor (1 = one file per pull).
    pub(crate) fn shared_chunked(
        lib_count: usize,
        weights: &[usize],
        shards: usize,
        chunk: usize,
    ) -> Self {
        let file_count = weights.len();
        debug_assert!(lib_count <= file_count);
        let shards = shards.clamp(1, file_count.max(1));
        let chunk = chunk.max(1);
        let total: usize = weights.iter().sum();
        // Fewer than `shards` files can each exceed an equal share; the
        // truncation only guards the arithmetic.
        let share = total / shards;
        let mut heavy: Vec<usize> = (0..file_count)
            .filter(|&file| shards > 1 && weights[file] > share)
            .collect();
        heavy.sort_by_key(|&file| (std::cmp::Reverse(weights[file]), file));
        heavy.truncate(shards - 1);
        let pinned = heavy
            .iter()
            .map(|&file| Lane {
                fixtures: (file >= lib_count).then_some(file).into_iter().collect(),
                libraries: (file < lib_count).then_some(file).into_iter().collect(),
                weight: weights[file],
                next_fixture: AtomicUsize::new(0),
                next_library: AtomicUsize::new(0),
            })
            .collect::<Vec<_>>();
        let mut pinned_files = heavy;
        pinned_files.sort_unstable();
        let pinned_total: usize = pinned.iter().map(|lane| lane.weight).sum();
        let pulling_shards = shards - pinned.len();
        let largest_unpinned = (0..file_count)
            .filter(|file| pinned_files.binary_search(file).is_err())
            .map(|file| weights[file])
            .max()
            .unwrap_or(0);
        Self {
            lib_count,
            file_count,
            lanes: Lanes::Shared {
                shards,
                pinned,
                pinned_files,
                reserved_nodes: ((total - pinned_total) / pulling_shards).max(largest_unpinned),
                next_fixture: AtomicUsize::new(lib_count),
                next_library: AtomicUsize::new(0),
                chunk,
                local_fixture: (0..shards)
                    .map(|_| (AtomicUsize::new(0), AtomicUsize::new(0)))
                    .collect(),
                local_library: (0..shards)
                    .map(|_| (AtomicUsize::new(0), AtomicUsize::new(0)))
                    .collect(),
            },
            steal: false,
            aborted: AtomicBool::new(false),
        }
    }

    /// One lane per non-empty share of the deterministic node-count
    /// partition of `weights` into at most `shards` shares.
    /// One lane per non-empty share of the deterministic partition of
    /// `weights` (node counts; index = Program file) into at most `shards`
    /// shares, by the mode of [`partition_mode_requested`]; `directories`
    /// gives each file's interned directory for the directory-preferring
    /// default (an empty slice falls back to the plain least-load spread).
    /// With `steal`, a shard whose share is exhausted takes files from the
    /// share with the most still waiting (see [`Self::steal_fixture`]).
    pub(crate) fn partitioned_with_directories(
        lib_count: usize,
        weights: &[usize],
        symbols: &[usize],
        directories: &[u32],
        shards: usize,
        steal: bool,
    ) -> Self {
        debug_assert!(lib_count <= weights.len());
        let files = match partition_mode_requested() {
            PartitionMode::Contiguous => partition_files_contiguous(weights, shards),
            PartitionMode::Blocks(per_shard) => partition_files_blocks(weights, shards, per_shard),
            PartitionMode::Directory(slack) if !directories.is_empty() => {
                partition_files_by_directory_with_cost(
                    weights,
                    symbols,
                    directories,
                    shards,
                    slack,
                    cost_model_requested(),
                )
            }
            PartitionMode::Directory(_) | PartitionMode::LeastLoad => {
                partition_files(weights, shards)
            }
        };
        let lanes = files
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
            steal,
            aborted: AtomicBool::new(false),
        }
    }

    /// Stop handing out files: a shard recorded an order-guard reason, so
    /// the serial replay is already certain.
    pub(crate) fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    pub(crate) fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
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
            Lanes::Shared {
                pinned,
                reserved_nodes,
                ..
            } => pinned
                .get(shard)
                .map_or(*reserved_nodes, |lane| lane.weight),
            Lanes::Partitioned(lanes) => lanes.get(shard).map_or(0, |lane| lane.weight),
        }
    }

    /// The next unchecked fixture for `shard`, in Program order, if any.
    pub(crate) fn next_fixture(&self, shard: usize) -> Option<usize> {
        if self.aborted() {
            return None;
        }
        match &self.lanes {
            Lanes::Shared {
                pinned,
                pinned_files,
                next_fixture,
                chunk,
                local_fixture,
                ..
            } => match pinned.get(shard) {
                Some(lane) => Self::pull(&lane.fixtures, &lane.next_fixture),
                None => Self::pull_unpinned(
                    next_fixture,
                    local_fixture.get(shard)?,
                    *chunk,
                    self.file_count,
                    pinned_files,
                ),
            },
            Lanes::Partitioned(lanes) => {
                let lane = lanes.get(shard)?;
                if let Some(file) = Self::pull(&lane.fixtures, &lane.next_fixture) {
                    return Some(file);
                }
                if !self.steal {
                    return None;
                }
                Self::steal_fixture(lanes, shard)
            }
        }
    }

    /// Work stealing over the deterministic partition: a shard whose own
    /// lane is exhausted takes the next file of the lane with the most
    /// files still waiting, so a share whose files check far slower than
    /// their node count predicted (type-level heavy code) does not hold
    /// the phase alone. Each lane's cursor hands every file out once; the
    /// emit phases attribute a file to the shard that checked it, so the
    /// stolen files follow their checker. `TSRS_SHARD_STEAL=0` keeps the
    /// static shares (see [`stealing_enabled`]).
    fn steal_fixture(lanes: &[Lane], shard: usize) -> Option<usize> {
        loop {
            let (victim, remaining) = lanes
                .iter()
                .enumerate()
                .filter(|&(index, _)| index != shard)
                .map(|(_, lane)| {
                    let remaining = lane
                        .fixtures
                        .len()
                        .saturating_sub(lane.next_fixture.load(Ordering::Relaxed));
                    (lane, remaining)
                })
                .max_by_key(|&(_, remaining)| remaining)?;
            if remaining == 0 {
                return None;
            }
            if let Some(file) = Self::pull(&victim.fixtures, &victim.next_fixture) {
                return Some(file);
            }
        }
    }

    /// The next unchecked library file for `shard`, in Program order, if any.
    pub(crate) fn next_library(&self, shard: usize) -> Option<usize> {
        if self.aborted() {
            return None;
        }
        match &self.lanes {
            Lanes::Shared {
                pinned,
                pinned_files,
                next_library,
                chunk,
                local_library,
                ..
            } => match pinned.get(shard) {
                Some(lane) => Self::pull(&lane.libraries, &lane.next_library),
                None => Self::pull_unpinned(
                    next_library,
                    local_library.get(shard)?,
                    *chunk,
                    self.lib_count,
                    pinned_files,
                ),
            },
            Lanes::Partitioned(lanes) => {
                let lane = lanes.get(shard)?;
                Self::pull(&lane.libraries, &lane.next_library)
            }
        }
    }

    fn pull(files: &[usize], next: &AtomicUsize) -> Option<usize> {
        files.get(next.fetch_add(1, Ordering::Relaxed)).copied()
    }

    /// The next file below `end` that is not pinned: from the shard's
    /// current chunk `local` (`(next, end)`), refilled with `chunk`
    /// consecutive files from the shared cursor when empty.
    fn pull_unpinned(
        next: &AtomicUsize,
        local: &(AtomicUsize, AtomicUsize),
        chunk: usize,
        end: usize,
        pinned_files: &[usize],
    ) -> Option<usize> {
        loop {
            let file = local.0.load(Ordering::Relaxed);
            if file < local.1.load(Ordering::Relaxed) {
                local.0.store(file + 1, Ordering::Relaxed);
                if pinned_files.binary_search(&file).is_err() {
                    return Some(file);
                }
                continue;
            }
            let base = next.fetch_add(chunk, Ordering::Relaxed);
            if base >= end {
                return None;
            }
            local.0.store(base, Ordering::Relaxed);
            local
                .1
                .store(base.saturating_add(chunk).min(end), Ordering::Relaxed);
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
    fn a_dominant_file_caps_the_shard_count() {
        // One file of a third of the program: four shards, never more.
        assert_eq!(dominant_file_shard_cap(941_600, 301_859), 4);
        // Exactly a quarter still dominates.
        assert_eq!(dominant_file_shard_cap(400_000, 100_000), 4);
        // A seventh of the nodes (hono's types.ts) does not: no cap.
        assert_eq!(dominant_file_shard_cap(137_692, 19_925), usize::MAX);
        // A program of equal small files keeps every shard.
        assert_eq!(dominant_file_shard_cap(300_000, 3_000), usize::MAX);
        // One file is one shard; nothing checked is no bound.
        assert_eq!(dominant_file_shard_cap(5_000, 5_000), 1);
        assert_eq!(dominant_file_shard_cap(0, 0), usize::MAX);
    }

    #[test]
    fn a_static_partition_takes_one_and_a_half_automatic_checkers() {
        // The automatic budget grows for a split nothing rebalances later; an
        // explicit count is kept as requested, and the growth stays capped.
        let automatic = CheckerBudget::automatic();
        let expected = (automatic.checkers() * 3 / 2).min(MAX_CHECKERS);
        assert_eq!(automatic.checkers_for_static_partition(), expected);
        assert!(automatic.checkers_for_static_partition() >= automatic.checkers());
        let explicit = CheckerBudget::new(NonZeroUsize::new(6).unwrap());
        assert_eq!(explicit.checkers_for_static_partition(), 6);
        let serial = CheckerBudget::serial();
        assert_eq!(serial.checkers_for_static_partition(), 1);
        let capped = CheckerBudget::new(NonZeroUsize::new(MAX_CHECKERS).unwrap());
        assert_eq!(capped.checkers_for_static_partition(), MAX_CHECKERS);
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
        let weights = [2; 16];
        let queue = ShardFileQueue::shared(4, &weights, 4);
        assert_eq!(queue.shard_count(), 4);
        // Sixteen equal files over four shards: nothing is pinned and the
        // mean share is the reservation.
        assert_eq!(queue.reserved_nodes(0), 8);
        let fixtures: Vec<usize> = std::iter::from_fn(|| queue.next_fixture(0)).collect();
        assert_eq!(fixtures, (4..16).collect::<Vec<_>>());
        assert_eq!(queue.next_fixture(1), None);
        let libraries: Vec<usize> = std::iter::from_fn(|| queue.next_library(3)).collect();
        assert_eq!(libraries, vec![0, 1, 2, 3]);
        assert_eq!(queue.next_library(0), None);
        let empty = ShardFileQueue::shared(0, &[], 8);
        assert_eq!(empty.shard_count(), 1);
        assert_eq!(empty.next_fixture(0), None);
        assert_eq!(empty.next_library(0), None);
    }

    #[test]
    fn shared_queue_pins_files_heavier_than_an_equal_share_to_their_own_shards() {
        // 28 nodes over 8 shards: files 2 (9) and 0 (4) exceed the share of
        // 3 and take shards 0 and 1, heaviest first; the other six shards
        // pull the rest in Program order, skipping the pinned files.
        let weights = [4, 1, 9, 2, 2, 2, 2, 2, 2, 2];
        let queue = ShardFileQueue::shared(3, &weights, 8);
        assert_eq!(queue.shard_count(), 8);
        assert_eq!(queue.next_fixture(0), None);
        assert_eq!(queue.next_library(0), Some(2));
        assert_eq!(queue.next_library(0), None);
        assert_eq!(queue.next_library(1), Some(0));
        assert_eq!(queue.next_library(1), None);
        assert_eq!(queue.reserved_nodes(0), 9);
        assert_eq!(queue.reserved_nodes(1), 4);
        let fixtures: Vec<usize> = std::iter::from_fn(|| queue.next_fixture(5)).collect();
        assert_eq!(fixtures, vec![3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(queue.next_fixture(2), None);
        let libraries: Vec<usize> = std::iter::from_fn(|| queue.next_library(7)).collect();
        assert_eq!(libraries, vec![1]);
        assert_eq!(queue.next_library(2), None);
        // (28 - 13) / 6 = 2, and no unpinned file is bigger.
        assert_eq!(queue.reserved_nodes(5), 2);
        // Two of three shards may be pinned; the third still pulls.
        let queue = ShardFileQueue::shared(0, &[100, 100, 1], 3);
        assert_eq!(queue.next_fixture(0), Some(0));
        assert_eq!(queue.next_fixture(1), Some(1));
        assert_eq!(queue.next_fixture(2), Some(2));
        assert_eq!(queue.next_fixture(2), None);
        // One shard: nothing is pinned.
        let queue = ShardFileQueue::shared(0, &[100, 1], 1);
        assert_eq!(queue.next_fixture(0), Some(0));
        assert_eq!(queue.next_fixture(0), Some(1));
        assert_eq!(queue.reserved_nodes(0), 101);
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
        let queue = ShardFileQueue::partitioned_with_directories(1, &weights, &[], &[], 2, false);
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
        let empty = ShardFileQueue::partitioned_with_directories(0, &[], &[], &[], 4, false);
        assert_eq!(empty.shard_count(), 1);
        assert_eq!(empty.next_fixture(0), None);
    }

    #[test]
    fn partitioned_queue_steals_from_the_share_with_the_most_waiting_files() {
        // File 0 is the library; seven equal weights alternate between the
        // two shares, so the fixtures split into [2, 4, 6] and [1, 3, 5].
        let weights = [1; 7];
        let queue = ShardFileQueue::partitioned_with_directories(1, &weights, &[], &[], 2, true);
        assert_eq!(
            partition_files(&weights, 2),
            vec![vec![0, 2, 4, 6], vec![1, 3, 5]]
        );
        // Shard 1 takes its own share, then the first waiting file of
        // shard 0's share.
        assert_eq!(queue.next_fixture(1), Some(1));
        assert_eq!(queue.next_fixture(1), Some(3));
        assert_eq!(queue.next_fixture(1), Some(5));
        assert_eq!(queue.next_fixture(1), Some(2));
        // Shard 0 continues its own share after the stolen file.
        assert_eq!(queue.next_fixture(0), Some(4));
        assert_eq!(queue.next_fixture(1), Some(6));
        assert_eq!(queue.next_fixture(0), None);
        assert_eq!(queue.next_fixture(1), None);
        // Libraries are never stolen.
        assert_eq!(queue.next_library(1), None);
        assert_eq!(queue.next_library(0), Some(0));
        assert_eq!(queue.next_library(0), None);
    }

    fn covers_every_file_once(assignment: &[Vec<usize>], files: usize) {
        let mut seen: Vec<usize> = assignment.iter().flatten().copied().collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..files).collect::<Vec<_>>());
        for share in assignment {
            assert!(share.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn directory_partition_clusters_a_directory_within_the_slack() {
        // Equal weights, two directories of four files: with a whole share
        // of slack each directory settles on one shard; with no slack the
        // files alternate like the plain least-load deal.
        let weights = [1; 8];
        let directories = [0, 0, 0, 0, 1, 1, 1, 1];
        let clustered = partition_files_by_directory(&weights, &directories, 2, 100);
        covers_every_file_once(&clustered, 8);
        assert_eq!(clustered, [vec![0, 1, 2, 3], vec![4, 5, 6, 7]]);
        let strict = partition_files_by_directory(&weights, &directories, 2, 0);
        covers_every_file_once(&strict, 8);
        assert_eq!(strict, partition_files(&weights, 2));
        // One directory holding every file gets no preference: the deal is
        // the plain least-load one whatever the slack.
        let single = partition_files_by_directory(&weights, &[0; 8], 2, 100);
        assert_eq!(single, partition_files(&weights, 2));
    }

    #[test]
    fn directory_partition_keeps_every_load_within_the_slack_of_the_lightest() {
        // Uneven weights and directories: whatever the preference chose,
        // no shard was dealt a file while more than the slack above the
        // lightest shard.
        let weights = [9, 1, 8, 2, 7, 3, 6, 4, 5, 5, 1, 1];
        let directories = [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 2, 2];
        let assignment = partition_files_by_directory(&weights, &directories, 3, 10);
        covers_every_file_once(&assignment, 12);
        let cost = |file: usize| cost_weight(weights[file]);
        let total: u64 = (0..12).map(cost).sum();
        let slack = total / 3 * 10 / 100;
        let loads: Vec<u64> = assignment
            .iter()
            .map(|share| share.iter().map(|&file| cost(file)).sum())
            .collect();
        let lightest = *loads.iter().min().unwrap();
        let heaviest_file = (0..12).map(cost).max().unwrap();
        assert!(loads
            .iter()
            .all(|&load| load <= lightest + slack + heaviest_file));
    }

    #[test]
    fn contiguous_and_block_partitions_cover_every_file_in_program_order() {
        let weights = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8];
        let contiguous = partition_files_contiguous(&weights, 3);
        covers_every_file_once(&contiguous, 12);
        assert_eq!(contiguous.len(), 3);
        for share in &contiguous {
            assert!(share.windows(2).all(|pair| pair[1] == pair[0] + 1));
        }
        let blocks = partition_files_blocks(&weights, 3, 2);
        covers_every_file_once(&blocks, 12);
        assert!(blocks.len() <= 3);
        assert!(partition_files_contiguous(&[], 4).is_empty());
        assert!(partition_files_blocks(&[], 4, 4).is_empty());
        assert_eq!(partition_files_by_directory(&[], &[], 4, 10).len(), 0);
    }

    #[test]
    fn chunked_shared_queue_hands_out_every_file_once_in_chunks() {
        // Three shards pull chunks of four from twenty fixtures behind two
        // libraries; file 9 is heavier than a share and pinned to shard 0.
        let mut weights = [1usize; 22];
        weights[9] = 100;
        let queue = ShardFileQueue::shared_chunked(2, &weights, 3, 4);
        assert_eq!(queue.shard_count(), 3);
        assert_eq!(queue.next_fixture(0), Some(9));
        assert_eq!(queue.next_fixture(0), None);
        let mut pulls: Vec<(usize, usize)> = Vec::new();
        for shard in [
            1, 2, 1, 1, 2, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2,
        ] {
            if let Some(file) = queue.next_fixture(shard) {
                pulls.push((shard, file));
            }
        }
        let mut files: Vec<usize> = pulls.iter().map(|&(_, file)| file).collect();
        files.sort_unstable();
        assert_eq!(files, (2..22).filter(|&file| file != 9).collect::<Vec<_>>());
        // The first chunk went to shard 1 whole: files 2..6 in order.
        assert_eq!(
            pulls
                .iter()
                .filter(|&&(shard, _)| shard == 1)
                .map(|&(_, file)| file)
                .take(4)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        assert_eq!(queue.next_fixture(1), None);
        assert_eq!(queue.next_fixture(2), None);
        let libraries: Vec<usize> = std::iter::from_fn(|| queue.next_library(1)).collect();
        assert_eq!(libraries, [0, 1]);
        assert_eq!(queue.next_library(2), None);
    }
}
