//! tsgo's checker budget and deterministic weighted FENNEL association, and
//! the fast preset's association by directory locality.
//!
//! A checker owns its caches and visits its associated files in Program order.
//! Under tsgo's association neither worker scheduling nor file check durations
//! can change ownership: the owner of a file is a function of the Program.
//!
//! The fast preset gives that up for the pool's width: its association is
//! the starting point, every checker starts with its heavy files, and a
//! checker that has run out of files takes unstarted ones from the back of the
//! lane with the most work left ([`ShardFileQueue::next_fixture`]). Which
//! checker checks a file then depends on how fast the others were, and with it
//! what a history-dependent program prints. The checker of every statement
//! range of a file checked in ranges stays a function of the Program.

use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// tsgo `newCheckerPoolWithTracing`: max(min(requested, files, 256), 1).
pub const MAX_CHECKERS: usize = 256;

/// How a Program's files are associated with its checkers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CheckerAssignment {
    /// tsgo's association (checkerpool.go): the default. With it the output
    /// is tsgo's for the same checker count.
    #[default]
    Tsgo,
    /// The fast preset: the files of one directory subtree share a checker
    /// (see [`associate_files_by_locality`]). Each checker repeats less of
    /// the other checkers' work; a diagnostic that depends on which files a
    /// checker saw first can differ from tsgo's.
    Locality,
}

/// The fast preset's pool: a checker for every core up to sixteen, beyond
/// that half the cores, at least four (tsgo's default) and at most
/// thirty-two — and never more than one checker per
/// [`FAST_FILES_PER_CHECKER`] type-checked files. Parsing and checking are
/// phases of their own, so a checker takes no core from the parser; what it
/// costs is memory and the declarations it resolves again, and a checker that
/// runs out of files takes over others' ([`ShardFileQueue::next_fixture`]).
/// The count follows the machine: a history-dependent program can print
/// something else on a machine with other cores.
const FAST_FILES_PER_CHECKER: usize = 32;
const FAST_MIN_CHECKERS: usize = 4;
const FAST_MAX_CHECKERS: usize = 32;
/// Up to this many cores every core gets a checker.
const FAST_CHECKER_PER_CORE_UP_TO: usize = 16;

/// A file heavier than one part in this many of a mean checker's share is
/// started first by its checker (see [`ShardFileQueue::take_from_the_fullest`]).
const HEAVY_SHARE_DIVISOR: u64 = 200;
/// From this many checkers on (or [`LARGE_STICKY_CHECKERS`] checkers on a
/// program of [`LARGE_STICKY_FILES`] checked files) a taking checker stays
/// with the lane it took from last while that lane has at least half the work
/// of the fullest: the back of a lane is one directory region, and every
/// region a taker enters costs it that region's modules.
const STICKY_CHECKERS: usize = 16;
const LARGE_STICKY_CHECKERS: usize = 8;
const LARGE_STICKY_FILES: usize = 24_000;
/// Between this many checkers and [`STICKY_CHECKERS`], at most one checker in
/// [`CHECKERS_PER_TAKER`] takes from one lane at a time: every taker of a
/// region builds that region's types for itself.
const TAKER_LIMIT_FROM_CHECKERS: usize = 8;
const CHECKERS_PER_TAKER: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckerBudget {
    checkers: NonZeroUsize,
    leak_states: bool,
    order_replay: bool,
    assignment: CheckerAssignment,
    /// The count is an upper bound that follows the Program's checked files.
    scaled: bool,
    /// The recorded costs of the Program's files, when its owner keeps a
    /// lane profile ([`crate::LaneProfile`]).
    lane_profile: Option<&'static crate::LaneProfile>,
}

impl Default for CheckerBudget {
    /// Embeddings that do not request a pool retain the serial API contract.
    fn default() -> Self {
        Self::serial()
    }
}

impl CheckerBudget {
    pub const fn serial() -> Self {
        Self {
            checkers: NonZeroUsize::MIN,
            leak_states: false,
            order_replay: false,
            assignment: CheckerAssignment::Tsgo,
            scaled: false,
            lane_profile: None,
        }
    }

    pub const fn new(checkers: NonZeroUsize) -> Self {
        let count = if checkers.get() > MAX_CHECKERS {
            MAX_CHECKERS
        } else {
            checkers.get()
        };
        Self {
            checkers: match NonZeroUsize::new(count) {
                Some(count) => count,
                None => NonZeroUsize::MIN,
            },
            ..Self::serial()
        }
    }

    pub const fn with_leaked_states(mut self, leak: bool) -> Self {
        self.leak_states = leak;
        self
    }

    pub const fn leaks_states(self) -> bool {
        self.leak_states
    }

    /// Optional serial replay for callers explicitly requesting serial output.
    /// The normal pool follows tsgo's per-checker type order.
    pub const fn with_order_replay(mut self, replay: bool) -> Self {
        self.order_replay = replay;
        self
    }

    pub const fn order_replay(self) -> bool {
        self.order_replay
    }

    /// tsgo's CLI default is four, independent of available hardware.
    pub fn automatic() -> Self {
        Self::new(NonZeroUsize::new(4).expect("four is nonzero"))
    }

    /// The fast preset without an explicit count on this machine: the
    /// locality association and a pool that follows the machine's cores and
    /// the Program's checked files.
    pub fn fast() -> Self {
        Self::fast_for(std::thread::available_parallelism().map_or(1, NonZeroUsize::get))
    }

    /// [`Self::fast`] for a machine of `cores` cores: one checker a core up
    /// to sixteen cores, then the larger of sixteen and half the cores,
    /// between four and thirty-two (4 on 4, 8 on 8, 10 on 10, 16 on 16 to 33,
    /// 32 on 64); [`Self::checkers_for`] lowers it for a small Program.
    pub fn fast_for(cores: usize) -> Self {
        let checkers = (cores / 2)
            .max(cores.min(FAST_CHECKER_PER_CORE_UP_TO))
            .clamp(FAST_MIN_CHECKERS, FAST_MAX_CHECKERS);
        Self {
            scaled: true,
            ..Self::new(NonZeroUsize::new(checkers).expect("four at least"))
                .with_assignment(CheckerAssignment::Locality)
        }
    }

    pub const fn with_assignment(mut self, assignment: CheckerAssignment) -> Self {
        self.assignment = assignment;
        self
    }

    pub const fn assignment(self) -> CheckerAssignment {
        self.assignment
    }

    /// A lane profile (tsrs-native, opt-in): the pool's checkers are
    /// levelled by the costs it holds, and the check records in it what it
    /// measured. The reference association only; not with an order replay.
    pub const fn with_lane_profile(mut self, profile: Option<&'static crate::LaneProfile>) -> Self {
        self.lane_profile = profile;
        self
    }

    pub(crate) const fn lane_profile(self) -> Option<&'static crate::LaneProfile> {
        self.lane_profile
    }

    /// The requested count; for the fast preset its upper bound.
    pub const fn checkers(self) -> usize {
        self.checkers.get()
    }

    /// The pool size for a Program of `files` files, `checked` of them type
    /// checked: tsgo's max(min(requested, files, 256), 1), where the fast
    /// preset requests one checker per [`FAST_FILES_PER_CHECKER`] checked
    /// files within its bounds.
    pub(crate) fn checkers_for(self, files: usize, checked: usize) -> usize {
        let requested = if self.scaled {
            (checked / FAST_FILES_PER_CHECKER).clamp(FAST_MIN_CHECKERS, self.checkers.get())
        } else {
            self.checkers.get()
        };
        requested.clamp(1, files.clamp(1, MAX_CHECKERS))
    }

    pub const fn is_sharded(self) -> bool {
        self.checkers.get() > 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AssociationPolicy {
    prioritize_sources: bool,
    source_multiplier: usize,
    penalty_multiplier: usize,
}

/// tsgo `getCheckerAssociationPolicy` (checkerpool.go).
fn association_policy(total: usize, declarations: usize, checkers: usize) -> AssociationPolicy {
    if declarations * checkers * 2 <= total {
        AssociationPolicy {
            prioritize_sources: true,
            source_multiplier: 1,
            penalty_multiplier: 12,
        }
    } else if checkers >= 4 {
        AssociationPolicy {
            prioritize_sources: false,
            source_multiplier: 4,
            penalty_multiplier: 16,
        }
    } else {
        AssociationPolicy {
            prioritize_sources: false,
            source_multiplier: 1,
            penalty_multiplier: 1,
        }
    }
}

pub(crate) fn base_weight(nodes: usize, text_bytes: usize) -> usize {
    (nodes + text_bytes / 100).max(1)
}

fn association_weights(base: &[usize], imports: &[usize]) -> Vec<usize> {
    let total_imports: usize = imports.iter().sum();
    let import_weight = base
        .iter()
        .sum::<usize>()
        .checked_div(total_imports)
        .map_or(0, |unit| unit.max(1));
    base.iter()
        .zip(imports)
        .map(|(&base, &imports)| base + imports * import_weight)
        .collect()
}

fn association_order(
    weights: &[usize],
    declarations: &[bool],
    prioritize: bool,
) -> Option<Vec<usize>> {
    prioritize.then(|| {
        let mut order = (0..weights.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|&file| {
            (declarations[file], std::cmp::Reverse(weights[file]), file)
        });
        order
    })
}

/// Direct port of tsgo `getCheckerAssociationsInOrder`. Keep the f64 operations
/// separate: Go's explicit conversions prohibit fused multiply/add rounding.
fn associations_in_order(
    weights: &[usize],
    adjacent: &[Vec<usize>],
    order: Option<&[usize]>,
    checkers: usize,
    penalty_multiplier: usize,
) -> Vec<usize> {
    if weights.is_empty() {
        return Vec::new();
    }
    let total: usize = weights.iter().sum();
    let max_file = weights.iter().copied().max().unwrap_or(0);
    let edges: usize = adjacent.iter().map(Vec::len).sum();
    let average = total.div_ceil(checkers);
    let max_checker = max_file.max(average + average / 100);
    let total_float = total as f64;
    let alpha = penalty_multiplier as f64 * (edges / 2) as f64 * (checkers as f64).sqrt()
        / (total_float * total_float.sqrt());
    let mut associations = vec![usize::MAX; weights.len()];
    let mut loads = vec![0usize; checkers];
    let mut neighbors = vec![0usize; checkers];
    for position in 0..weights.len() {
        let file = order.map_or(position, |order| order[position]);
        neighbors.fill(0);
        for &other in &adjacent[file] {
            let checker = associations[other];
            if checker != usize::MAX {
                neighbors[checker] += 1;
            }
        }
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for (checker, &load) in loads.iter().enumerate() {
            if load + weights[file] > max_checker {
                continue;
            }
            let old = load as f64;
            let new = (load + weights[file]) as f64;
            let new_penalty = new * new.sqrt();
            let old_penalty = old * old.sqrt();
            let penalty = alpha * (new_penalty - old_penalty);
            let score = neighbors[checker] as f64 - penalty;
            if score > best_score
                || (score == best_score && best.is_none_or(|best| load < loads[best]))
            {
                best = Some(checker);
                best_score = score;
            }
        }
        let checker = best.unwrap_or_else(|| {
            // Ascending index breaks ties, including fallback when no bin fits.
            (0..checkers).min_by_key(|&checker| loads[checker]).unwrap()
        });
        associations[file] = checker;
        loads[checker] += weights[file];
    }
    associations
}

pub(crate) fn associate_files(
    base_weights: &[usize],
    imports: &[usize],
    declarations: &[bool],
    adjacent: &[Vec<usize>],
    checkers: usize,
) -> Vec<usize> {
    let checkers = checkers.clamp(1, base_weights.len().clamp(1, MAX_CHECKERS));
    if checkers == 1 {
        return vec![0; base_weights.len()];
    }
    let total = base_weights.iter().sum();
    let declaration_weight = base_weights
        .iter()
        .zip(declarations)
        .filter_map(|(&weight, &declaration)| declaration.then_some(weight))
        .sum();
    let policy = association_policy(total, declaration_weight, checkers);
    let base = base_weights
        .iter()
        .zip(declarations)
        .map(|(&weight, &declaration)| {
            weight
                * if declaration {
                    1
                } else {
                    policy.source_multiplier
                }
        })
        .collect::<Vec<_>>();
    let weights = association_weights(&base, imports);
    let order = association_order(&weights, declarations, policy.prioritize_sources);
    associations_in_order(
        &weights,
        adjacent,
        order.as_deref(),
        checkers,
        policy.penalty_multiplier,
    )
}

/// A checker's recorded cost may exceed the mean by one part in this many
/// before files are moved off it.
const COST_TOLERANCE_PARTS: usize = 50;

/// A levelling must take at least one part in this many off the costliest
/// checker, or the reference's association stays: the cost measure cannot
/// tell a smaller difference from none, and every moved file makes its new
/// checker resolve what the file uses.
const COST_GAIN_PARTS: usize = 25;

/// A file that fits a move, with its rank: the import neighbours it gains,
/// its cost, and the lower file index among equals.
type FittingFile = ((i64, usize, std::cmp::Reverse<usize>), usize);

/// Levels the reference's association by recorded costs (a lane profile,
/// [`crate::lane_profile`]) with as few moves as it takes, inside the
/// reference's own peak budget ([`capped_level`]): while the costliest
/// checker is more than the tolerance above the level, one of its files goes
/// to the lightest checker that has room under its cap — a file that fits
/// what the one has too much and the other too little, the one with the most
/// import neighbours gained (those on the taking checker less those it
/// leaves), the costliest among equals. A Program whose checkers are level by
/// cost keeps the reference's association, file for file. `movable[file]` is
/// false for a file no checker type checks.
pub(crate) fn level_by_cost(
    associations: &mut [usize],
    costs: &[usize],
    movable: &[bool],
    adjacent: &[Vec<usize>],
    checkers: usize,
) {
    if checkers < 2 {
        return;
    }
    let mut loads = vec![0usize; checkers];
    for (&checker, &cost) in associations.iter().zip(costs) {
        loads[checker] += cost;
    }
    let mean = loads.iter().sum::<usize>() / checkers;
    let (caps, level) = capped_level(&loads);
    let limit = level + mean / COST_TOLERANCE_PARTS;
    let costliest_before = loads.iter().copied().max().unwrap_or(0);
    let reference = associations.to_vec();
    'moves: loop {
        // The lowest index among equals.
        let heaviest = (0..checkers)
            .max_by_key(|&checker| (loads[checker], std::cmp::Reverse(checker)))
            .expect("two checkers at least");
        if loads[heaviest] <= limit {
            break;
        }
        let mut takers = (0..checkers)
            .filter(|&checker| checker != heaviest && loads[checker] < caps[checker])
            .collect::<Vec<_>>();
        takers.sort_by_key(|&checker| (loads[checker], checker));
        for lightest in takers {
            let amount = (loads[heaviest] - level)
                .min(level.min(caps[lightest]).saturating_sub(loads[lightest]));
            // The import neighbours a file gains by the move: those on the
            // taking checker less those it leaves.
            let gain = |file: usize| {
                adjacent[file]
                    .iter()
                    .map(|&other| {
                        i64::from(associations[other] == lightest)
                            - i64::from(associations[other] == heaviest)
                    })
                    .sum::<i64>()
            };
            let mut fitting: Option<FittingFile> = None;
            let mut cheapest: Option<usize> = None;
            for file in 0..associations.len() {
                if associations[file] != heaviest || !movable[file] || costs[file] == 0 {
                    continue;
                }
                if cheapest.is_none_or(|other| (costs[file], file) < (costs[other], other)) {
                    cheapest = Some(file);
                }
                if costs[file] <= amount {
                    let key = (gain(file), costs[file], std::cmp::Reverse(file));
                    if fitting.is_none_or(|(best, _)| key > best) {
                        fitting = Some((key, file));
                    }
                }
            }
            // Nothing fits: the cheapest file, if the pair's costlier checker
            // is cheaper after its move and the taking one stays under its cap.
            let file = fitting.map(|(_, file)| file).or_else(|| {
                cheapest.filter(|&file| {
                    loads[lightest] + costs[file] < loads[heaviest]
                        && loads[lightest] + costs[file] <= caps[lightest]
                })
            });
            let Some(file) = file else {
                continue;
            };
            associations[file] = lightest;
            loads[heaviest] -= costs[file];
            loads[lightest] += costs[file];
            continue 'moves;
        }
        break;
    }
    let costliest_after = loads.iter().copied().max().unwrap_or(0);
    if costliest_before.saturating_sub(costliest_after) < costliest_before / COST_GAIN_PARTS {
        associations.copy_from_slice(&reference);
    }
}

/// The peak budget of a dealing, in its own cost unit: per checker the load
/// it may reach, and the level the checkers under no cap share.
///
/// A checker that ends returns its tables while the others still check, so a
/// pool's peak goes with the largest `(checkers - rank) * load` over the loads
/// in ascending order (that many checkers are alive when the one of that rank
/// ends), not with their sum. A dealing that is level by cost has no early
/// end and the highest peak of all (measured 2026-11-18, four checkers: next
/// 19 % faster with 54 MiB more, zod 0.5 % faster with 41 MiB more). The
/// budget is that largest product for the loads as they are; the checker of a
/// rank may not grow above `budget / (checkers - rank)`, and the level is the
/// load at which the capped checkers and the others sum to the total.
fn capped_level(loads: &[usize]) -> (Vec<usize>, usize) {
    let checkers = loads.len();
    let total = loads.iter().sum::<usize>();
    let mut order = (0..checkers).collect::<Vec<_>>();
    order.sort_by_key(|&checker| (loads[checker], checker));
    let budget = order
        .iter()
        .enumerate()
        .map(|(rank, &checker)| (checkers - rank) * loads[checker])
        .max()
        .unwrap_or(0);
    let mut caps = vec![0usize; checkers];
    for (rank, &checker) in order.iter().enumerate() {
        caps[checker] = budget / (checkers - rank);
    }
    // The caps rise with the rank: the lowest ranks sit at theirs, the others
    // share what is left.
    let mut level = total / checkers;
    let mut capped = 0usize;
    for (rank, &checker) in order.iter().enumerate() {
        level = (total - capped) / (checkers - rank);
        if level <= caps[checker] {
            break;
        }
        capped += caps[checker];
    }
    (caps, level)
}

/// tsgo `checkerAssociationSourceFileWeightMultiplier`: every file the
/// locality association weighs is a file a checker works for.
const SOURCE_FILE_WEIGHT_MULTIPLIER: usize = 4;
/// A directory subtree of at most this fraction of a mean checker load is
/// one group of the locality association.
const LOCALITY_GROUP_FRACTION: usize = 4;
/// The groups are placed for their import edges first: FENNEL's unscaled
/// balance penalty.
const LOCALITY_PENALTY_MULTIPLIER: usize = 1;
/// The weight of the module-affinity term beside a group's import edges.
const MODULE_AFFINITY: f64 = 1.0;
/// Passes of the refinement after the placement; it converges in a few more,
/// with nothing measurable left to gain.
const REFINEMENT_PASSES: usize = 3;

/// A declaration file that weighs at least this percentage of a mean checker
/// load is checked in statement ranges on several checkers, each range about
/// one in [`SPLIT_PIECE_DIVISOR`] of a mean load. The weight of a generated
/// declaration file can understate its check several times over (the 2.9 MB
/// declaration file of schema types in Next.js: 5 % of the weight, a fifth
/// of a one-checker check), so a file well under a whole load can be what
/// the check waits for; a lighter file costs about its weight, and cutting
/// it only makes more checkers resolve what its statements use.
const SPLIT_MIN_SHARE_PERCENT: usize = 40;
const SPLIT_PIECE_DIVISOR: usize = 8;

/// What a checker pays to hold a module: it resolves the part of the module
/// its files use, which grows slower than the module.
fn module_worth(nodes: usize) -> f64 {
    (nodes as f64).sqrt()
}

/// A file the fast preset checks in statement ranges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SplitFile {
    pub(crate) file: usize,
    /// Contiguous ranges of the file's top-level statements, each with the
    /// checker that checks it. The first is the range of the file's own
    /// checker; no two ranges share a checker.
    pub(crate) pieces: Vec<(Range<usize>, usize)>,
}

/// The fast preset's association: every file's checker, the files checked in
/// statement ranges in ascending file index, and what every file weighed in
/// the placement (a file checked in ranges: its own checker's range; a file
/// no checker works for: nothing).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LocalityAssociation {
    pub(crate) checkers: Vec<usize>,
    pub(crate) splits: Vec<SplitFile>,
    pub(crate) weights: Vec<usize>,
}

/// At most `count` contiguous ranges of statements with about equal text,
/// given every statement's text length; none when there would be fewer than
/// two.
fn statement_pieces(lengths: &[u32], count: usize) -> Vec<Range<usize>> {
    let count = count.min(lengths.len());
    if count < 2 {
        return Vec::new();
    }
    let total = lengths.iter().map(|&length| u64::from(length)).sum::<u64>();
    let mut pieces = Vec::with_capacity(count);
    let mut start = 0;
    let mut text = 0u64;
    for (index, &length) in lengths.iter().enumerate() {
        text += u64::from(length);
        // Range k closes once the text so far reaches k / count of the
        // whole, while a statement remains for every range still to come.
        let closing = pieces.len() + 1;
        if closing < count
            && text * count as u64 >= total * closing as u64
            && lengths.len() - (index + 1) >= count - closing
        {
            pieces.push(start..index + 1);
            start = index + 1;
        }
    }
    pieces.push(start..lengths.len());
    if pieces.len() < 2 {
        return Vec::new();
    }
    pieces
}

/// The fast preset's association. What an extra checker repeats is the
/// declarations its files resolve that another checker's files resolve too,
/// and the files of one directory subtree (a feature folder, a package)
/// resolve largely the same ones. So:
///
/// 1. only a file a checker works for carries weight (`checked`): tsgo's base
///    weight as a source file plus its normalized import count. A declaration
///    or JSON file that is not type checked would otherwise take a share of
///    a checker's load for nothing; such files go to checker 0;
/// 2. the weighted files are sorted by name, and every file belongs to the
///    shallowest directory subtree that weighs at most
///    1/[`LOCALITY_GROUP_FRACTION`] of a mean checker load (a file directly
///    inside a heavier directory is a group of its own);
/// 3. the groups are placed by tsgo's FENNEL step, heaviest first. A group's
///    affinity to a checker is the file-level import edges to the groups
///    already there, plus a module term: a group's modules are its files and
///    the files they import (`targets`), a module is worth the square root
///    of its node count (`nodes`), and the term is the worth of the group's
///    modules the checker already holds, scaled so that holding all of them
///    counts as much as the group's own import edges;
/// 4. a few refinement passes then move single groups, lightest first, to
///    the checker where the worth of the modules held, summed over the
///    checkers, falls most, never above the load cap of the placement.
/// 5. an unsplit file that exceeds a mean syntax load keeps its checker;
///    other whole-file groups on that checker move, heaviest first, to the
///    lightest remaining checker. Import fanout cannot hide a file that
///    already exceeds a syntax share. Statement-range owners stay fixed.
///
/// A file can be more than any checker should carry alone: one that weighs
/// [`SPLIT_MIN_SHARE_PERCENT`] of a mean load (it is then a group of its own)
/// and whose statements can be checked apart (`statement_lengths` gives the
/// text length of each, or nothing) is cut into ranges of statements. Its
/// group keeps the first range; every other range is placed like a group
/// without edges or modules — on the lightest checker at its turn — and the
/// ranges of one file go to different checkers and stay there.
///
/// Import edges alone miss two things: a module that no checker type checks
/// (a library declaration file carries no weight, so it is in no group) and
/// the size of the module an edge leads to. What the checkers repeat follows
/// the modules they hold in common.
///
/// Deterministic: names are compared as ASCII-lowercased text (then by
/// Program index), groups are numbered in that order, every sum runs in
/// ascending file index and every tie is broken by index. `names` are those
/// lowercased names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn associate_files_by_locality(
    names: &[String],
    base_weights: &[usize],
    imports: &[usize],
    checked: &[bool],
    adjacent: &[Vec<usize>],
    targets: &[Vec<usize>],
    nodes: &[usize],
    statement_lengths: &dyn Fn(usize) -> Vec<u32>,
    checkers: usize,
) -> LocalityAssociation {
    let checkers = checkers.clamp(1, base_weights.len().clamp(1, MAX_CHECKERS));
    let mut associations = vec![0; base_weights.len()];
    if checkers == 1 {
        return LocalityAssociation {
            weights: vec![0; associations.len()],
            checkers: associations,
            splits: Vec::new(),
        };
    }
    let source_weights = base_weights
        .iter()
        .zip(checked)
        .map(|(&weight, &checked)| {
            if checked {
                weight * SOURCE_FILE_WEIGHT_MULTIPLIER
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let checked_imports = imports
        .iter()
        .zip(checked)
        .map(|(&imports, &checked)| if checked { imports } else { 0 })
        .collect::<Vec<_>>();
    let weights = association_weights(&source_weights, &checked_imports);
    let mut order = (0..weights.len())
        .filter(|&file| weights[file] > 0)
        .collect::<Vec<_>>();
    if order.is_empty() {
        return LocalityAssociation {
            weights: vec![0; associations.len()],
            checkers: associations,
            splits: Vec::new(),
        };
    }
    order.sort_unstable_by(|&a, &b| names[a].cmp(&names[b]).then(a.cmp(&b)));
    let total: usize = order.iter().map(|&file| weights[file]).sum();
    let threshold = total / (checkers * LOCALITY_GROUP_FRACTION);

    // The weight of every directory subtree, keyed by the directory's name.
    let mut subtrees = rustc_hash::FxHashMap::<&str, usize>::default();
    for &file in &order {
        let name = names[file].as_str();
        for (end, _) in name.match_indices('/') {
            *subtrees.entry(&name[..end]).or_default() += weights[file];
        }
    }
    let mut groups = rustc_hash::FxHashMap::<&str, usize>::default();
    let mut group_of = vec![usize::MAX; weights.len()];
    let mut group_weights = Vec::new();
    let mut group_sizes = Vec::new();
    for &file in &order {
        let name = names[file].as_str();
        let key = name
            .match_indices('/')
            .map(|(end, _)| &name[..end])
            .find(|directory| subtrees[directory] <= threshold)
            .unwrap_or(name);
        let next = group_weights.len();
        let group = *groups.entry(key).or_insert(next);
        if group == next {
            group_weights.push(0);
            group_sizes.push(0usize);
        }
        group_of[file] = group;
        group_weights[group] += weights[file];
        group_sizes[group] += 1;
    }
    // One adjacency entry per file-level import edge and direction.
    let mut group_adjacent = vec![Vec::new(); group_weights.len()];
    for &file in &order {
        for &other in &adjacent[file] {
            let (group, other) = (group_of[file], group_of[other]);
            if other != usize::MAX && other != group {
                group_adjacent[group].push(other);
            }
        }
    }
    // A group's modules: its files and the files they import, in ascending
    // file index.
    let mut group_modules = vec![Vec::new(); group_weights.len()];
    for &file in &order {
        let modules = &mut group_modules[group_of[file]];
        modules.push(file);
        modules.extend_from_slice(&targets[file]);
    }
    for modules in &mut group_modules {
        modules.sort_unstable();
        modules.dedup();
    }
    // The files checked in statement ranges, in ascending file index: the
    // file's group keeps the first range's weight, the other ranges follow
    // the groups; `exclusive` numbers the ranges of one file alike.
    let share = total / checkers;
    let piece_weight = (share / SPLIT_PIECE_DIVISOR).max(1);
    let mut exclusive = vec![0u32; group_weights.len()];
    let mut split_groups = Vec::<(usize, Vec<Range<usize>>, usize)>::new();
    // What every file weighs on its own checker: a file checked in ranges,
    // its first range.
    let mut own_weights = weights.clone();
    for file in 0..weights.len() {
        let group = group_of[file];
        if group == usize::MAX
            || group_sizes[group] != 1
            || weights[file] * 100 < share * SPLIT_MIN_SHARE_PERCENT
        {
            continue;
        }
        let lengths = statement_lengths(file);
        let pieces = statement_pieces(&lengths, weights[file].div_ceil(piece_weight).min(checkers));
        if pieces.is_empty() {
            continue;
        }
        let text = lengths
            .iter()
            .map(|&length| u128::from(length))
            .sum::<u128>()
            .max(1);
        let id = u32::try_from(split_groups.len() + 1).expect("fewer splits than files");
        let first_other = group_weights.len();
        for (index, piece) in pieces.iter().enumerate() {
            let piece_text = lengths[piece.clone()]
                .iter()
                .map(|&length| u128::from(length))
                .sum::<u128>();
            let weight = ((weights[file] as u128 * piece_text / text) as usize).max(1);
            if index == 0 {
                group_weights[group] = weight;
                exclusive[group] = id;
                own_weights[file] = weight;
            } else {
                group_weights.push(weight);
                group_adjacent.push(Vec::new());
                group_modules.push(Vec::new());
                exclusive.push(id);
            }
        }
        split_groups.push((file, pieces, first_other));
    }
    let worth = nodes
        .iter()
        .map(|&nodes| module_worth(nodes))
        .collect::<Vec<_>>();
    let mut placed = place_groups(
        &group_weights,
        &group_adjacent,
        &group_modules,
        &worth,
        &exclusive,
        checkers,
    );
    // Imports contribute another syntax budget to the placement weights.
    // A whole file can exceed a syntax share yet fit beside other groups
    // under that combined cap. Keep those neighbours off its critical path.
    let syntax_share = source_weights.iter().sum::<usize>() / checkers;
    let oversized = source_weights
        .iter()
        .enumerate()
        .filter_map(|(file, &weight)| {
            let group = group_of[file];
            (group != usize::MAX
                && group_sizes[group] == 1
                && exclusive[group] == 0
                && weight > syntax_share)
                .then_some(group)
        })
        .collect::<Vec<_>>();
    isolate_oversized_groups(
        &group_weights,
        &oversized,
        &exclusive,
        checkers,
        &mut placed,
    );
    for &file in &order {
        associations[file] = placed[group_of[file]];
    }
    let splits = split_groups
        .into_iter()
        .map(|(file, pieces, first_other)| SplitFile {
            file,
            pieces: pieces
                .into_iter()
                .enumerate()
                .map(|(index, statements)| {
                    let group = if index == 0 {
                        group_of[file]
                    } else {
                        first_other + index - 1
                    };
                    (statements, placed[group])
                })
                .collect(),
        })
        .collect();
    LocalityAssociation {
        checkers: associations,
        splits,
        weights: own_weights,
    }
}

/// An unsplit file larger than a syntax share keeps its original checker;
/// its other whole-file groups move to the lightest unreserved checker.
/// Statement ranges stay fixed so their distinct-owner constraint survives.
fn isolate_oversized_groups(
    weights: &[usize],
    oversized: &[usize],
    exclusive: &[u32],
    checkers: usize,
    placed: &mut [usize],
) {
    if oversized.is_empty() {
        return;
    }
    let mut reserved = vec![false; checkers];
    let mut stays = vec![false; weights.len()];
    for &group in oversized {
        reserved[placed[group]] = true;
        stays[group] = true;
    }
    if reserved.iter().all(|&reserved| reserved) {
        return;
    }
    let mut loads = vec![0usize; checkers];
    for (&weight, &checker) in weights.iter().zip(placed.iter()) {
        loads[checker] += weight;
    }
    let mut order = (0..weights.len())
        .filter(|&group| reserved[placed[group]] && !stays[group] && exclusive[group] == 0)
        .collect::<Vec<_>>();
    order.sort_unstable_by_key(|&group| (std::cmp::Reverse(weights[group]), group));
    for group in order {
        let from = placed[group];
        let to = (0..checkers)
            .filter(|&checker| !reserved[checker])
            .min_by_key(|&checker| loads[checker])
            .expect("an unreserved checker exists");
        loads[from] -= weights[group];
        loads[to] += weights[group];
        placed[group] = to;
    }
}

/// Steps 3 and 4 of [`associate_files_by_locality`]: tsgo's FENNEL step over
/// the groups with the module term, then the refinement. Groups with the
/// same nonzero `exclusive` number (the statement ranges of one file) are
/// placed on different checkers and not moved afterwards; there are never
/// more of them than checkers.
fn place_groups(
    weights: &[usize],
    adjacent: &[Vec<usize>],
    modules: &[Vec<usize>],
    worth: &[f64],
    exclusive: &[u32],
    checkers: usize,
) -> Vec<usize> {
    let files = worth.len();
    let total: usize = weights.iter().sum();
    let heaviest = weights.iter().copied().max().unwrap_or(0);
    let edges: usize = adjacent.iter().map(Vec::len).sum();
    let average = total.div_ceil(checkers);
    let cap = heaviest.max(average + average / 100);
    let total_float = total as f64;
    let alpha = LOCALITY_PENALTY_MULTIPLIER as f64 * (edges / 2) as f64 * (checkers as f64).sqrt()
        / (total_float * total_float.sqrt());
    let group_worth = modules
        .iter()
        .map(|modules| modules.iter().map(|&module| worth[module]).sum::<f64>())
        .collect::<Vec<_>>();
    // held[module * checkers + checker]: the checker's groups that hold the
    // module. A group's sums over its modules are taken for every checker in
    // one pass, adding the module's worth or zero: no branch on the data, and
    // each checker's sum still runs in ascending module index.
    let mut held = vec![0u32; files * checkers];
    // taken[(number - 1) * checkers + checker]: the checker has a range of
    // that file.
    let numbers = exclusive.iter().copied().max().unwrap_or(0) as usize;
    let mut taken = vec![false; numbers * checkers];
    let mut placed = vec![usize::MAX; weights.len()];
    let mut loads = vec![0usize; checkers];
    let mut neighbors = vec![0usize; checkers];
    let mut sums = vec![0.0f64; checkers];
    let mut order = (0..weights.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|&group| (std::cmp::Reverse(weights[group]), group));
    for &group in &order {
        neighbors.fill(0);
        for &other in &adjacent[group] {
            let checker = placed[other];
            if checker != usize::MAX {
                neighbors[checker] += 1;
            }
        }
        let scale = if group_worth[group] > 0.0 {
            MODULE_AFFINITY * (adjacent[group].len() + 1) as f64 / group_worth[group]
        } else {
            0.0
        };
        // The worth of the group's modules each checker already holds.
        sums.fill(0.0);
        for &module in &modules[group] {
            let holders = &held[module * checkers..][..checkers];
            for (sum, &count) in sums.iter_mut().zip(holders) {
                *sum += if count != 0 { worth[module] } else { 0.0 };
            }
        }
        let taken = exclusive[group]
            .checked_sub(1)
            .map(|number| &mut taken[number as usize * checkers..][..checkers]);
        let free = |checker: usize| taken.as_ref().is_none_or(|taken| !taken[checker]);
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for (checker, &load) in loads.iter().enumerate() {
            if load + weights[group] > cap || !free(checker) {
                continue;
            }
            let old = load as f64;
            let new = (load + weights[group]) as f64;
            let new_penalty = new * new.sqrt();
            let old_penalty = old * old.sqrt();
            let penalty = alpha * (new_penalty - old_penalty);
            let affinity = neighbors[checker] as f64 + scale * sums[checker];
            let score = affinity - penalty;
            if score > best_score
                || (score == best_score && best.is_none_or(|best| load < loads[best]))
            {
                best = Some(checker);
                best_score = score;
            }
        }
        let checker = best.unwrap_or_else(|| {
            // Ascending index breaks ties, including fallback when no bin fits.
            (0..checkers)
                .filter(|&checker| free(checker))
                .min_by_key(|&checker| loads[checker])
                .expect("a file has no more ranges than there are checkers")
        });
        if let Some(taken) = taken {
            taken[checker] = true;
        }
        placed[group] = checker;
        loads[checker] += weights[group];
        for &module in &modules[group] {
            held[module * checkers + checker] += 1;
        }
    }
    let mut sequence = (0..weights.len()).collect::<Vec<_>>();
    sequence.sort_unstable_by_key(|&group| (weights[group], group));
    for _ in 0..REFINEMENT_PASSES {
        let mut moved = false;
        for &group in &sequence {
            if exclusive[group] != 0 {
                continue;
            }
            let from = placed[group];
            // What leaves `from` with the group (the modules only it holds
            // there), and what each other checker would newly hold.
            let mut released = 0.0f64;
            sums.fill(0.0);
            for &module in &modules[group] {
                let holders = &held[module * checkers..][..checkers];
                released += if holders[from] == 1 {
                    worth[module]
                } else {
                    0.0
                };
                for (sum, &count) in sums.iter_mut().zip(holders) {
                    *sum += if count == 0 { worth[module] } else { 0.0 };
                }
            }
            let mut best = None;
            let mut best_gain = 1e-9;
            for to in 0..checkers {
                if to == from || loads[to] + weights[group] > cap {
                    continue;
                }
                let gain = released - sums[to];
                if gain > best_gain {
                    best = Some(to);
                    best_gain = gain;
                }
            }
            let Some(to) = best else {
                continue;
            };
            for &module in &modules[group] {
                held[module * checkers + from] -= 1;
                held[module * checkers + to] += 1;
            }
            loads[from] -= weights[group];
            loads[to] += weights[group];
            placed[group] = to;
            moved = true;
        }
        if !moved {
            break;
        }
    }
    placed
}

/// The files of every checker, in the order it visits them. Empty checkers
/// are retained, as in tsgo's pool.
///
/// A lane's files are its checker's, and under tsgo's association nobody else
/// touches them. Under the fast preset ([`Self::take_from_the_fullest`]) a
/// lane is a queue with two ends: its checker takes at the front, and a
/// checker that has no file left takes at the back of another lane.
#[derive(Debug)]
pub(crate) struct ShardFileQueue {
    lanes: Vec<Lane>,
    /// The statements its own checker checks of a file checked in ranges.
    owned_statements: rustc_hash::FxHashMap<usize, Range<usize>>,
    aborted: AtomicBool,
    /// The checkers measure what every fixture's check costs (a lane
    /// profile is written after the check).
    measures_costs: bool,
    /// The fast preset's rules for a checker without files, when it takes
    /// from other lanes.
    taking: Option<Taking>,
    /// The fast preset's history rules hold for the checkers of this queue
    /// (`CheckerState::canonical_history`): whatever its lane count, and
    /// whether or not its checkers take from each other.
    canonical_history: bool,
}

/// How checkers without files take from other lanes.
#[derive(Clone, Copy, Debug)]
struct Taking {
    /// The most checkers that take from one lane at a time; zero for no
    /// limit.
    takers_per_lane: usize,
    /// A taking checker stays with its lane while that has half the work of
    /// the fullest.
    sticky: bool,
}

/// Where a checker without files of its own takes from: its place among a
/// lane's takers, and the lane it took from last.
#[derive(Debug)]
pub(crate) struct TakeCursor {
    lane: usize,
    last: usize,
}

impl Default for TakeCursor {
    fn default() -> Self {
        Self {
            lane: usize::MAX,
            last: usize::MAX,
        }
    }
}

#[derive(Debug, Default)]
struct Lane {
    fixtures: Vec<usize>,
    libraries: Vec<usize>,
    /// Statement ranges of other lanes' files, checked before the lane's own
    /// files.
    pieces: Vec<(usize, Range<usize>)>,
    weight: usize,
    next_library: AtomicUsize,
    /// The fixtures nobody has taken yet.
    untaken: Mutex<Untaken>,
    /// `fixtures[..kept]` are never taken by another checker: the files
    /// checked in ranges, whose other ranges other checkers hold.
    kept: usize,
    /// `weights_before[i]`: the scheduling weight of `fixtures[..i]`; empty
    /// while no checker takes from another.
    weights_before: Vec<u64>,
    /// The weight of the untaken fixtures another checker may take. A read
    /// may be a moment old: it only chooses the lane to try.
    takeable: AtomicU64,
    /// The checkers that take from this lane now.
    takers: Mutex<Vec<usize>>,
}

/// `fixtures[front..back]` of a lane.
#[derive(Debug, Default)]
struct Untaken {
    front: usize,
    back: usize,
}

impl Lane {
    fn lock_untaken(&self) -> std::sync::MutexGuard<'_, Untaken> {
        self.untaken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_takers(&self) -> std::sync::MutexGuard<'_, Vec<usize>> {
        self.takers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Publishes what another checker may still take of `untaken`.
    fn publish_takeable(&self, untaken: &Untaken) {
        if self.weights_before.is_empty() {
            return;
        }
        let from = untaken.front.max(self.kept);
        let weight = if from < untaken.back {
            self.weights_before[untaken.back] - self.weights_before[from]
        } else {
            0
        };
        self.takeable.store(weight, Ordering::Relaxed);
    }

    /// The lane's own checker takes its next file.
    fn take_front(&self) -> Option<usize> {
        let mut untaken = self.lock_untaken();
        if untaken.front >= untaken.back {
            return None;
        }
        let file = self.fixtures[untaken.front];
        untaken.front += 1;
        self.publish_takeable(&untaken);
        Some(file)
    }

    /// Another checker takes the lane's last untaken file, unless its own
    /// checker keeps it.
    fn take_back(&self) -> Option<usize> {
        let mut untaken = self.lock_untaken();
        if untaken.front.max(self.kept) >= untaken.back {
            return None;
        }
        untaken.back -= 1;
        self.publish_takeable(&untaken);
        Some(self.fixtures[untaken.back])
    }

    fn takeable(&self) -> u64 {
        self.takeable.load(Ordering::Relaxed)
    }

    /// Whether `checker` may take from this lane: it does already, or fewer
    /// than `limit` checkers do (zero: no limit).
    fn admits(&self, checker: usize, limit: usize) -> bool {
        if limit == 0 {
            return true;
        }
        let takers = self.lock_takers();
        takers.len() < limit || takers.contains(&checker)
    }

    /// Makes `checker` one of this lane's takers if [`Self::admits`] it.
    fn admit(&self, checker: usize, limit: usize) -> bool {
        if limit == 0 {
            return true;
        }
        let mut takers = self.lock_takers();
        if takers.contains(&checker) {
            return true;
        }
        if takers.len() >= limit {
            return false;
        }
        takers.push(checker);
        true
    }

    fn leave(&self, checker: usize) {
        self.lock_takers().retain(|&taker| taker != checker);
    }
}

impl ShardFileQueue {
    pub(crate) fn associated(
        lib_count: usize,
        node_counts: &[usize],
        associations: &[usize],
        checkers: usize,
    ) -> Self {
        let checkers = checkers.clamp(1, node_counts.len().clamp(1, MAX_CHECKERS));
        let mut lanes = (0..checkers).map(|_| Lane::default()).collect::<Vec<_>>();
        for (file, &checker) in associations.iter().enumerate() {
            let lane = &mut lanes[checker];
            lane.weight += node_counts[file];
            if file < lib_count {
                lane.libraries.push(file);
            } else {
                lane.fixtures.push(file);
            }
        }
        for lane in &mut lanes {
            lane.untaken = Mutex::new(Untaken {
                front: 0,
                back: lane.fixtures.len(),
            });
        }
        Self {
            lanes,
            owned_statements: rustc_hash::FxHashMap::default(),
            aborted: AtomicBool::new(false),
            measures_costs: false,
            taking: None,
            canonical_history: false,
        }
    }

    /// From now on the checkers measure what every fixture's check costs.
    pub(crate) fn measure_costs(&mut self) {
        self.measures_costs = true;
    }

    pub(crate) fn measures_costs(&self) -> bool {
        self.measures_costs
    }

    /// Adds the statement ranges of `splits` (in ascending file index, so
    /// every lane's ranges are too). A range reserves its share of the
    /// file's nodes on its lane.
    pub(crate) fn split(&mut self, splits: &[SplitFile], node_counts: &[usize]) {
        for split in splits {
            let Some(((own, _), others)) = split.pieces.split_first() else {
                continue;
            };
            self.owned_statements.insert(split.file, own.clone());
            for (statements, checker) in others {
                let lane = &mut self.lanes[*checker];
                lane.pieces.push((split.file, statements.clone()));
                lane.weight += node_counts[split.file] / split.pieces.len();
            }
        }
    }

    /// The fast preset's scheduling of a whole-Program check, after
    /// [`Self::split`]: a checker that has run out of files takes unstarted
    /// ones from the back of the lane with the most work left
    /// ([`Self::next_fixture`]). `weights[file]` is what the file weighed in
    /// the placement, `checked` the number of files a checker works for.
    ///
    /// A file cannot be cut, so the check ends when the checker with the
    /// costliest file has finished it. Every lane therefore starts with its
    /// heavy files — those above one part in [`HEAVY_SHARE_DIVISOR`] of a
    /// mean lane's weight, heaviest first — so that such a file overlaps the
    /// other checkers' work instead of following it. The other files keep the
    /// Program's order: it keeps a checker's caches warm, and it leaves the
    /// lane's back, where others take, one directory region.
    ///
    /// A file checked in ranges stays with its checker, at the lane's very
    /// front: the other checkers hold its other ranges, and no checker may
    /// hold two ranges of one file.
    pub(crate) fn take_from_the_fullest(&mut self, weights: &[usize], checked: usize) {
        let checkers = self.lanes.len();
        if checkers < 2 {
            return;
        }
        let weight = |file: usize| weights[file] as u64;
        let total = self
            .lanes
            .iter()
            .flat_map(|lane| &lane.fixtures)
            .map(|&file| weight(file))
            .sum::<u64>();
        let active = self
            .lanes
            .iter()
            .filter(|lane| !lane.fixtures.is_empty())
            .count()
            .max(1) as u64;
        let heavy_above = total / (active * HEAVY_SHARE_DIVISOR);
        let owned_statements = &self.owned_statements;
        for lane in &mut self.lanes {
            let (kept, rest): (Vec<usize>, Vec<usize>) = lane
                .fixtures
                .iter()
                .partition(|&&file| owned_statements.contains_key(&file));
            let (mut heavy, light): (Vec<usize>, Vec<usize>) = rest
                .into_iter()
                .partition(|&file| weight(file) > heavy_above);
            // Stable: equal weights keep the Program's order.
            heavy.sort_by_key(|&file| std::cmp::Reverse(weight(file)));
            lane.kept = kept.len();
            lane.fixtures = kept;
            lane.fixtures.extend(heavy);
            lane.fixtures.extend(light);
            lane.weights_before = std::iter::once(0)
                .chain(lane.fixtures.iter().scan(0u64, |sum, &file| {
                    *sum += weight(file);
                    Some(*sum)
                }))
                .collect();
            let untaken = Untaken {
                front: 0,
                back: lane.fixtures.len(),
            };
            lane.publish_takeable(&untaken);
            lane.untaken = Mutex::new(untaken);
        }
        self.taking = Some(Taking {
            takers_per_lane: if (TAKER_LIMIT_FROM_CHECKERS..STICKY_CHECKERS).contains(&checkers) {
                checkers / CHECKERS_PER_TAKER
            } else {
                0
            },
            sticky: checkers >= STICKY_CHECKERS
                || (checkers >= LARGE_STICKY_CHECKERS && checked >= LARGE_STICKY_FILES),
        });
    }

    /// The checkers of this queue keep the fast preset's history rules.
    pub(crate) fn keep_canonical_history(&mut self) {
        self.canonical_history = true;
    }

    /// Whether the checkers of this queue keep the fast preset's history
    /// rules ([`Self::keep_canonical_history`]).
    pub(crate) fn canonical_history(&self) -> bool {
        self.canonical_history
    }

    /// The statement ranges of other lanes' files this lane checks first.
    pub(crate) fn pieces(&self, shard: usize) -> &[(usize, Range<usize>)] {
        &self.lanes[shard].pieces
    }

    /// The statements `file`'s own checker checks, if the file is checked in
    /// ranges.
    pub(crate) fn owned_statements(&self, file: usize) -> Option<Range<usize>> {
        self.owned_statements.get(&file).cloned()
    }

    pub(crate) fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    pub(crate) fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    pub(crate) fn shard_count(&self) -> usize {
        self.lanes.len()
    }

    pub(crate) fn reserved_nodes(&self, shard: usize) -> usize {
        self.lanes[shard].weight
    }

    /// Adds `nodes` to every lane's reservation: the share of the files no
    /// checker owns work for, which the locality association puts on lane 0.
    pub(crate) fn reserve_on_every_lane(&mut self, nodes: usize) {
        for lane in &mut self.lanes {
            lane.weight += nodes;
        }
    }

    /// The next file for the checker of lane `shard`, and whether it comes
    /// from another lane.
    ///
    /// A checker takes its own files from the front of its lane. Under the
    /// fast preset's scheduling a checker without files then takes the last
    /// untaken file of the lane with the most weight left among those that
    /// admit it (the lower lane among equals) and stays one of that lane's
    /// takers until it moves on; with the sticky rule it keeps the lane it
    /// took from last while that has at least half the fullest's weight left.
    /// When only files without weight are left (files no checker works for),
    /// anyone takes them. `None`: nothing is left for this checker — the
    /// lanes with work have their takers and their own checkers.
    pub(crate) fn next_fixture(
        &self,
        shard: usize,
        cursor: &mut TakeCursor,
    ) -> Option<(usize, bool)> {
        if self.aborted() {
            return None;
        }
        if let Some(file) = self.lanes[shard].take_front() {
            return Some((file, false));
        }
        let taking = self.taking?;
        let leave = |cursor: &mut TakeCursor| {
            if cursor.lane != usize::MAX {
                self.lanes[cursor.lane].leave(shard);
                cursor.lane = usize::MAX;
            }
        };
        loop {
            if self.aborted() {
                leave(&mut *cursor);
                return None;
            }
            let others = || {
                self.lanes
                    .iter()
                    .enumerate()
                    .filter(move |&(other, _)| other != shard)
            };
            if self.lanes.iter().all(|lane| lane.takeable() == 0) {
                leave(&mut *cursor);
                return others()
                    .find_map(|(_, lane)| lane.take_back())
                    .map(|file| (file, true));
            }
            let fullest = others()
                .filter(|(_, lane)| lane.admits(shard, taking.takers_per_lane))
                .map(|(other, lane)| (lane.takeable(), std::cmp::Reverse(other)))
                .max();
            let Some((left, std::cmp::Reverse(mut from))) = fullest else {
                leave(&mut *cursor);
                return None;
            };
            if left == 0 {
                leave(&mut *cursor);
                return None;
            }
            if taking.sticky {
                if cursor.last != usize::MAX {
                    let last = &self.lanes[cursor.last];
                    if last.admits(shard, taking.takers_per_lane) && last.takeable() * 2 >= left {
                        from = cursor.last;
                    }
                }
                cursor.last = from;
            }
            if from != cursor.lane {
                if !self.lanes[from].admit(shard, taking.takers_per_lane) {
                    continue;
                }
                leave(&mut *cursor);
                cursor.lane = from;
            }
            if let Some(file) = self.lanes[from].take_back() {
                return Some((file, true));
            }
        }
    }

    pub(crate) fn next_library(&self, shard: usize) -> Option<usize> {
        if self.aborted() {
            return None;
        }
        let lane = &self.lanes[shard];
        lane.libraries
            .get(lane.next_library.fetch_add(1, Ordering::Relaxed))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsgo_budget_and_policy() {
        assert_eq!(CheckerBudget::automatic().checkers(), 4);
        assert_eq!(CheckerBudget::serial().checkers(), 1);
        assert_eq!(
            CheckerBudget::new(NonZeroUsize::new(1000).unwrap()).checkers(),
            256
        );
        assert_eq!(base_weight(100, 2500), 125);
        assert_eq!(associate_files(&[1], &[0], &[false], &[vec![]], 256), [0]);
        assert!(associate_files(&[], &[], &[], &[], 4).is_empty());
        assert_eq!(
            association_policy(1000, 125, 4),
            AssociationPolicy {
                prioritize_sources: true,
                source_multiplier: 1,
                penalty_multiplier: 12,
            }
        );
        assert_eq!(
            association_policy(1000, 126, 4),
            AssociationPolicy {
                prioritize_sources: false,
                source_multiplier: 4,
                penalty_multiplier: 16,
            }
        );
        assert_eq!(association_policy(1000, 400, 2).penalty_multiplier, 1);
        assert_eq!(
            association_weights(&[100, 50, 25], &[0, 1, 3]),
            [100, 93, 154]
        );
        assert_eq!(
            association_order(&[5, 10, 7, 2], &[true, false, false, true], true),
            Some(vec![1, 2, 0, 3])
        );
        assert_eq!(association_order(&[5], &[false], false), None);
    }

    #[test]
    fn tsgo_graph_goldens() {
        // checkerpool_test.go TestGetCheckerAssociations, same inputs/outputs.
        let empty = vec![Vec::new(); 6];
        assert_eq!(
            associations_in_order(&[1; 6], &empty, None, 3, 1),
            [0, 1, 2, 0, 1, 2]
        );
        assert_eq!(
            associations_in_order(&[1, 3, 2], &empty[..3], None, 2, 1),
            [0, 1, 0]
        );
        let graph = vec![
            vec![1, 2],
            vec![0, 2],
            vec![0, 1],
            vec![4, 5],
            vec![3, 5],
            vec![3, 4],
        ];
        assert_eq!(
            associations_in_order(&[1; 6], &graph, None, 2, 1),
            [0, 0, 0, 1, 1, 1]
        );
        assert!(associations_in_order(&[], &[], None, 4, 16).is_empty());
    }

    #[test]
    fn fast_budget_follows_the_program() {
        assert_eq!(
            CheckerBudget::fast().assignment(),
            CheckerAssignment::Locality
        );
        assert_eq!(
            CheckerBudget::automatic().assignment(),
            CheckerAssignment::Tsgo
        );
        // The machine's part: a checker a core up to sixteen cores, then the
        // larger of sixteen and half the cores, between 4 and 32.
        let pool = |cores: usize| CheckerBudget::fast_for(cores).checkers();
        assert_eq!(
            [1, 4, 8, 10, 16, 24, 33, 48, 64, 128].map(pool),
            [4, 4, 8, 10, 16, 16, 16, 24, 32, 32]
        );
        assert_eq!(
            CheckerBudget::fast().checkers(),
            pool(std::thread::available_parallelism().map_or(1, NonZeroUsize::get))
        );
        // The Program's part: one checker per 32 checked files, at least 4,
        // never more than the machine's count or the files.
        let fast = CheckerBudget::fast_for(12);
        assert_eq!(fast.checkers_for(10_270, 9_243), 12);
        assert_eq!(fast.checkers_for(739, 440), 12);
        assert_eq!(fast.checkers_for(600, 352), 11);
        assert_eq!(fast.checkers_for(362, 187), 5);
        assert_eq!(fast.checkers_for(249, 78), 4);
        assert_eq!(fast.checkers_for(3, 3), 3);
        assert_eq!(fast.checkers_for(0, 0), 1);
        assert_eq!(CheckerBudget::fast_for(10).checkers_for(10_270, 9_243), 10);
        assert_eq!(CheckerBudget::fast_for(64).checkers_for(10_270, 9_243), 32);
        assert_eq!(CheckerBudget::fast_for(64).checkers_for(739, 440), 13);
        // An explicit count is kept under either association.
        let eight = CheckerBudget::new(NonZeroUsize::new(8).unwrap());
        assert_eq!(eight.checkers_for(1000, 10), 8);
        assert_eq!(
            eight
                .with_assignment(CheckerAssignment::Locality)
                .checkers_for(1000, 10),
            8
        );
        assert_eq!(CheckerBudget::automatic().checkers_for(1000, 0), 4);
        assert_eq!(CheckerBudget::automatic().checkers_for(2, 2), 2);
    }

    /// The inputs of one association: lowercased names, base weights, import
    /// counts, checked flags, undirected adjacency, import targets and node
    /// counts.
    struct LocalityProgram {
        names: Vec<String>,
        base: Vec<usize>,
        imports: Vec<usize>,
        checked: Vec<bool>,
        adjacent: Vec<Vec<usize>>,
        targets: Vec<Vec<usize>>,
    }

    impl LocalityProgram {
        /// `edges`: (importer, imported). The node counts are the base
        /// weights.
        fn new(
            names: &[&str],
            base: &[usize],
            imports: &[usize],
            unchecked: &[usize],
            edges: &[(usize, usize)],
        ) -> Self {
            let mut checked = vec![true; names.len()];
            for &file in unchecked {
                checked[file] = false;
            }
            let mut adjacent = vec![Vec::new(); names.len()];
            let mut targets = vec![Vec::new(); names.len()];
            for &(importer, imported) in edges {
                adjacent[importer].push(imported);
                adjacent[imported].push(importer);
                targets[importer].push(imported);
            }
            Self {
                names: names.iter().map(|&name| name.to_owned()).collect(),
                base: base.to_vec(),
                imports: imports.to_vec(),
                checked,
                adjacent,
                targets,
            }
        }

        fn associate(&self, checkers: usize) -> Vec<usize> {
            let association = self.associate_with(checkers, &|_| Vec::new());
            assert!(association.splits.is_empty());
            association.checkers
        }

        /// The association when `statement_lengths` says which files can be
        /// checked in statement ranges.
        fn associate_with(
            &self,
            checkers: usize,
            statement_lengths: &dyn Fn(usize) -> Vec<u32>,
        ) -> LocalityAssociation {
            associate_files_by_locality(
                &self.names,
                &self.base,
                &self.imports,
                &self.checked,
                &self.adjacent,
                &self.targets,
                &self.base,
                statement_lengths,
                checkers,
            )
        }

        /// The same program with its files listed in `order`.
        fn permuted(&self, order: &[usize]) -> Self {
            let position = |file: usize| order.iter().position(|&f| f == file).unwrap();
            let moved = |lists: &[Vec<usize>]| {
                order
                    .iter()
                    .map(|&file| lists[file].iter().map(|&other| position(other)).collect())
                    .collect::<Vec<Vec<usize>>>()
            };
            Self {
                names: order.iter().map(|&file| self.names[file].clone()).collect(),
                base: order.iter().map(|&file| self.base[file]).collect(),
                imports: order.iter().map(|&file| self.imports[file]).collect(),
                checked: order.iter().map(|&file| self.checked[file]).collect(),
                adjacent: moved(&self.adjacent),
                targets: moved(&self.targets),
            }
        }
    }

    /// Ten files: directories `a` (three files) and `b` (two) are light
    /// enough to be one group each; `c/big.ts` and `d/huge.ts` outweigh the
    /// group bound, so they and `c/small.ts` beside them are groups of their
    /// own, as is `root.ts`; `lib.d.ts` is not checked.
    fn locality_program() -> LocalityProgram {
        LocalityProgram::new(
            &[
                "/p/a/one.ts",
                "/p/a/two.ts",
                "/p/a/deep/three.ts",
                "/p/b/one.ts",
                "/p/b/two.ts",
                "/p/c/big.ts",
                "/p/c/small.ts",
                "/p/lib.d.ts",
                "/p/root.ts",
                "/p/d/huge.ts",
            ],
            &[100, 80, 60, 120, 40, 5000, 30, 500, 50, 4000],
            &[0, 0, 0, 0, 0, 1, 0, 7, 0, 1],
            &[7],
            &[
                (0, 1),
                (2, 0),
                (3, 4),
                (3, 0),
                (5, 6),
                (8, 0),
                (8, 3),
                (8, 5),
                (9, 6),
                (7, 0),
            ],
        )
    }

    #[test]
    fn locality_keeps_directory_subtrees_together() {
        let program = locality_program();
        // The two heaviest groups go first, each to an empty checker; the
        // directory groups follow their import edges.
        assert_eq!(program.associate(2), [1, 1, 1, 1, 1, 0, 1, 0, 1, 1]);
        assert_eq!(program.associate(3), [2, 2, 2, 2, 2, 0, 2, 0, 2, 1]);
        assert_eq!(program.associate(4), [2, 2, 2, 2, 2, 0, 3, 0, 2, 1]);
        for checkers in 2..=4 {
            let associations = program.associate(checkers);
            // A directory group is never split, and the unchecked file
            // carries no weight: it stays on checker 0.
            assert!(associations[..3].iter().all(|&c| c == associations[0]));
            assert_eq!(associations[3], associations[4]);
            assert_eq!(associations[7], 0);
        }
        assert_eq!(program.associate(1), [0; 10]);
    }

    #[test]
    fn locality_gathers_the_importers_of_a_module() {
        // Eight files of four directories, each a group of its own, import
        // one of two library files that no checker type checks. No import
        // edge joins two groups, so the edges alone alternate the files over
        // the checkers; the module term puts the importers of one library on
        // one checker.
        let program = LocalityProgram::new(
            &[
                "/q/a/x.ts",
                "/q/a/y.ts",
                "/q/b/x.ts",
                "/q/b/y.ts",
                "/q/c/x.ts",
                "/q/c/y.ts",
                "/q/d/x.ts",
                "/q/d/y.ts",
                "/q/lib/big.d.ts",
                "/q/lib/other.d.ts",
                "/q/heavy1.ts",
                "/q/heavy2.ts",
            ],
            &[
                100, 100, 100, 100, 100, 100, 100, 100, 9000, 9000, 3000, 3000,
            ],
            &[1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0],
            &[8, 9],
            &[
                (0, 8),
                (1, 8),
                (2, 9),
                (3, 9),
                (4, 8),
                (5, 8),
                (6, 9),
                (7, 9),
            ],
        );
        let associations = program.associate(2);
        assert_eq!(associations, [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1]);
        for importer in [0, 1, 4, 5] {
            assert_eq!(associations[importer], associations[0]);
        }
        for importer in [2, 3, 6, 7] {
            assert_eq!(associations[importer], associations[2]);
        }
        assert_ne!(associations[0], associations[2]);
        assert_eq!(program.associate(3), [2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 1]);
    }

    #[test]
    fn a_syntax_dominant_file_keeps_its_checker_without_import_weight() {
        let program = LocalityProgram::new(
            &[
                "/p/heavy.ts",
                "/p/d0/small.ts",
                "/p/d1/small.ts",
                "/p/d2/small.ts",
                "/p/d3/small.ts",
                "/p/d4/small.ts",
                "/p/d5/small.ts",
                "/p/barrel.ts",
            ],
            &[320, 100, 100, 100, 100, 100, 100, 80],
            &[0, 0, 0, 0, 0, 0, 0, 40],
            &[],
            &[(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)],
        );
        // Syntax weighs 1,000, so the 320-node file exceeds a four-way
        // share even though its zero imports hide that in the combined
        // weights. Affinity must not serialize all its importers behind it.
        assert_eq!(program.associate(4), [1, 2, 3, 2, 3, 2, 3, 0]);
    }

    #[test]
    fn oversized_owners_receive_no_other_whole_file_groups() {
        let weights = [100, 12, 9, 8, 7, 6, 5, 4];
        let mut placed = [1, 1, 1, 1, 0, 2, 3, 3];
        isolate_oversized_groups(&weights, &[0], &[0; 8], 4, &mut placed);
        assert_eq!(placed, [1, 2, 0, 3, 0, 2, 3, 3]);
        let once = placed;
        isolate_oversized_groups(&weights, &[0], &[0; 8], 4, &mut placed);
        assert_eq!(placed, once);
        isolate_oversized_groups(&weights, &[], &[0; 8], 4, &mut placed);
        assert_eq!(placed, once);
    }

    #[test]
    fn oversized_owner_evacuation_keeps_statement_range_owners() {
        let weights = [100, 12, 9, 8, 7, 6, 5, 4];
        let mut placed = [1, 1, 1, 1, 0, 2, 3, 0];
        let exclusive = [0, 2, 0, 0, 0, 0, 2, 2];
        let before = placed;
        isolate_oversized_groups(&weights, &[0], &exclusive, 4, &mut placed);
        for group in [0, 1, 6, 7] {
            assert_eq!(placed[group], before[group]);
        }
        assert_ne!(placed[2], 1);
        assert_ne!(placed[3], 1);
    }

    #[test]
    fn locality_depends_on_the_program_only() {
        let program = locality_program();
        let expected = program.associate(3);
        // The same program listed in another order gives every file the same
        // checker: groups are numbered by name, not by Program index.
        let order = [9, 3, 0, 7, 5, 1, 8, 2, 6, 4];
        let permuted = program.permuted(&order).associate(3);
        for (index, &file) in order.iter().enumerate() {
            assert_eq!(permuted[index], expected[file], "file {file}");
        }
        // No checked file: everything stays on checker 0.
        let mut unchecked = locality_program();
        unchecked.checked.fill(false);
        assert_eq!(unchecked.associate(3), [0; 10]);
        let empty =
            associate_files_by_locality(&[], &[], &[], &[], &[], &[], &[], &|_| Vec::new(), 4);
        assert_eq!(empty, LocalityAssociation::default());
    }

    #[test]
    fn statement_ranges_have_about_equal_text() {
        assert_eq!(
            statement_pieces(&[10; 8], 4),
            [0..2, 2..4, 4..6, 6..8],
            "equal statements"
        );
        // A range closes at the statement that brings the text so far to its
        // part of the whole.
        assert_eq!(statement_pieces(&[100, 1, 1, 1], 2), [0..1, 1..4]);
        assert_eq!(statement_pieces(&[1, 100, 1, 1], 2), [0..2, 2..4]);
        assert_eq!(
            statement_pieces(&[5, 30, 5, 5, 30, 5, 5, 30, 5], 3),
            [0..3, 3..6, 6..9]
        );
        // No more ranges than statements, and a statement for every range.
        assert_eq!(statement_pieces(&[7, 7, 7], 8), [0..1, 1..2, 2..3]);
        assert_eq!(statement_pieces(&[50, 50, 1, 1], 3), [0..1, 1..2, 2..4]);
        // The text that would close the first range comes with the last
        // statement: nothing to cut.
        assert!(statement_pieces(&[1, 1, 100], 3).is_empty());
        // Fewer than two ranges is no split.
        assert!(statement_pieces(&[10, 10, 10], 1).is_empty());
        assert!(statement_pieces(&[10], 4).is_empty());
        assert!(statement_pieces(&[], 4).is_empty());
    }

    /// A declaration file of 40 % of the program beside six source files in
    /// three directories, each file a group of its own at four checkers.
    fn heavy_declaration_program() -> LocalityProgram {
        LocalityProgram::new(
            &[
                "/p/big.d.ts",
                "/p/a/x.ts",
                "/p/a/y.ts",
                "/p/b/x.ts",
                "/p/b/y.ts",
                "/p/c/x.ts",
                "/p/c/y.ts",
            ],
            &[4000, 1000, 1000, 1000, 1000, 1000, 1000],
            &[0; 7],
            &[],
            &[],
        )
    }

    #[test]
    fn a_heavy_declaration_file_is_checked_in_ranges_on_different_checkers() {
        let program = heavy_declaration_program();
        // Whole, the file is a checker's entire load and more.
        assert_eq!(program.associate(4), [0, 1, 2, 3, 1, 2, 3]);
        // With eight equal statements it is 1.6 mean loads, so it is cut
        // into as many ranges as there are checkers. The file's group keeps
        // the first range; the others are placed in turn on the lightest
        // checker that has none of the file yet.
        let lengths = |file: usize| if file == 0 { vec![10; 8] } else { Vec::new() };
        let association = program.associate_with(4, &lengths);
        assert_eq!(association.checkers, [0, 0, 1, 2, 3, 1, 2]);
        assert_eq!(
            association.splits,
            [SplitFile {
                file: 0,
                pieces: vec![(0..2, 0), (2..4, 3), (4..6, 1), (6..8, 2)],
            }]
        );
        let split = &association.splits[0];
        assert_eq!(split.pieces[0].1, association.checkers[0]);
        let mut checkers = split
            .pieces
            .iter()
            .map(|&(_, checker)| checker)
            .collect::<Vec<_>>();
        checkers.sort_unstable();
        checkers.dedup();
        assert_eq!(checkers.len(), split.pieces.len());

        // A file whose statements cannot be checked apart stays whole, as
        // does one below the bound (here 0.18 mean loads).
        let single = |file: usize| if file == 0 { vec![80] } else { Vec::new() };
        assert!(program.associate_with(4, &single).splits.is_empty());
        let light = LocalityProgram::new(
            &["/p/small.d.ts", "/p/a/x.ts", "/p/a/y.ts", "/p/b/x.ts"],
            &[300, 1000, 1000, 1000],
            &[0; 4],
            &[],
            &[],
        );
        let lengths = |file: usize| if file == 0 { vec![10; 8] } else { Vec::new() };
        assert!(light.associate_with(2, &lengths).splits.is_empty());

        // The queue: the owner keeps the file with its own range; every
        // other range is on its checker's lane, before that lane's files.
        let nodes = [4000, 1000, 1000, 1000, 1000, 1000, 1000];
        let mut queue = ShardFileQueue::associated(0, &nodes, &association.checkers, 4);
        queue.split(&association.splits, &nodes);
        assert_eq!(queue.owned_statements(0), Some(0..2));
        assert_eq!(queue.owned_statements(1), None);
        assert!(queue.pieces(0).is_empty());
        assert_eq!(queue.pieces(1), [(0, 4..6)]);
        assert_eq!(queue.pieces(2), [(0, 6..8)]);
        assert_eq!(queue.pieces(3), [(0, 2..4)]);
        let mut cursor = TakeCursor::default();
        assert_eq!(queue.next_fixture(0, &mut cursor), Some((0, false)));
        assert_eq!(queue.next_fixture(0, &mut cursor), Some((1, false)));
        assert_eq!(queue.next_fixture(0, &mut cursor), None);
        assert_eq!(queue.reserved_nodes(3), 1000 + 4000 / 4);

        // Under the preset's scheduling the file checked in ranges stays with
        // its checker, at the front of its lane, whatever the others weigh:
        // nobody takes it, and the lane's other file can be taken.
        let mut queue = ShardFileQueue::associated(0, &nodes, &association.checkers, 4);
        queue.split(&association.splits, &nodes);
        queue.take_from_the_fullest(&association.weights, 7);
        let lane = association.checkers[0];
        let other = (0..4).find(|&checker| checker != lane).unwrap();
        let mut taker = TakeCursor::default();
        let mut taken = Vec::new();
        // The taker's own files first, then everything it may take.
        while let Some((file, from_another_lane)) = queue.next_fixture(other, &mut taker) {
            if from_another_lane {
                taken.push(file);
            }
        }
        assert!(!taken.contains(&0), "a range owner's file was taken");
        assert_eq!(taken.len(), 6 - queue.lanes[other].fixtures.len());
        let mut owner = TakeCursor::default();
        assert_eq!(queue.next_fixture(lane, &mut owner), Some((0, false)));
        assert_eq!(queue.next_fixture(lane, &mut owner), None);
    }

    #[test]
    fn ownership_is_independent_of_lane_schedule() {
        let expected = [0, 1, 2, 0, 1, 2];
        for schedule in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
            let queue = ShardFileQueue::associated(2, &[1; 6], &expected, 3);
            for checker in schedule {
                let mut files = Vec::new();
                while let Some(file) = queue.next_library(checker) {
                    files.push(file);
                }
                // Without the preset's scheduling no lane's file is another
                // checker's, however the checkers are scheduled.
                let mut cursor = TakeCursor::default();
                while let Some((file, from_another_lane)) = queue.next_fixture(checker, &mut cursor)
                {
                    assert!(!from_another_lane);
                    files.push(file);
                }
                assert_eq!(
                    files,
                    (0..6)
                        .filter(|&file| expected[file] == checker)
                        .collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(
            ShardFileQueue::associated(0, &[1], &[0], 8).shard_count(),
            1
        );
    }

    /// A queue of `lanes` lanes over files of the given weights, each file on
    /// the lane `associations` names, under the preset's scheduling.
    fn taking_queue(weights: &[usize], associations: &[usize], lanes: usize) -> ShardFileQueue {
        let mut queue = ShardFileQueue::associated(0, &vec![1; weights.len()], associations, lanes);
        queue.take_from_the_fullest(weights, weights.len());
        queue
    }

    /// Everything `checker` gets from `queue`, with whether each file came
    /// from another lane.
    fn drain(queue: &ShardFileQueue, checker: usize) -> Vec<(usize, bool)> {
        let mut cursor = TakeCursor::default();
        std::iter::from_fn(|| queue.next_fixture(checker, &mut cursor)).collect()
    }

    #[test]
    fn a_lane_starts_with_its_heavy_files_and_keeps_the_rest_in_program_order() {
        // Two lanes; lane 0 holds files 0..6. The weights sum to 2,400, so a
        // file above 2,400 / (2 * 200) = 6 is heavy: file 4 (900), then 1 and
        // 2 (500 each, in the Program's order) go first; 0, 3 and 5 keep
        // their order.
        let weights = [5, 500, 500, 6, 900, 3, 486];
        let queue = taking_queue(&weights, &[0, 0, 0, 0, 0, 0, 1], 2);
        assert_eq!(queue.lanes[0].fixtures, [4, 1, 2, 0, 3, 5]);
        assert_eq!(queue.lanes[1].fixtures, [6]);
        // The owner takes at the front, the other checker at the back.
        let mut owner = TakeCursor::default();
        let mut taker = TakeCursor::default();
        assert_eq!(queue.next_fixture(0, &mut owner), Some((4, false)));
        assert_eq!(queue.next_fixture(1, &mut taker), Some((6, false)));
        assert_eq!(queue.next_fixture(1, &mut taker), Some((5, true)));
        assert_eq!(queue.next_fixture(1, &mut taker), Some((3, true)));
        assert_eq!(queue.next_fixture(0, &mut owner), Some((1, false)));
        assert_eq!(queue.next_fixture(0, &mut owner), Some((2, false)));
        assert_eq!(queue.next_fixture(1, &mut taker), Some((0, true)));
        assert_eq!(queue.next_fixture(0, &mut owner), None);
        assert_eq!(queue.next_fixture(1, &mut taker), None);
    }

    #[test]
    fn a_checker_without_files_takes_from_the_lane_with_the_most_left() {
        // Lane 0 holds 300, lane 1 holds 500, lane 2 nothing: checker 2 takes
        // from lane 1 until lane 0 has more left, the lower lane among equals.
        let weights = [100, 100, 100, 250, 150, 100];
        let queue = taking_queue(&weights, &[0, 0, 0, 1, 1, 1], 3);
        // Every file is above the bound (800 / (2 * 200) = 2): heaviest first
        // within a lane, the Program's order among equals.
        assert_eq!(queue.lanes[0].fixtures, [0, 1, 2]);
        assert_eq!(queue.lanes[1].fixtures, [3, 4, 5]);
        let taken = drain(&queue, 2);
        // 500 against 300: file 5; 400 against 300: file 4; 250 against 300:
        // lane 0's file 2; 250 against 200: file 3; then lane 0's rest.
        assert_eq!(
            taken,
            [
                (5, true),
                (4, true),
                (2, true),
                (3, true),
                (1, true),
                (0, true)
            ]
        );
        assert_eq!(drain(&queue, 0), []);
        assert_eq!(drain(&queue, 1), []);
    }

    #[test]
    fn files_without_weight_are_taken_when_nothing_else_is_left() {
        // Lane 0 holds two files no checker works for and one of weight 10.
        let weights = [0, 0, 10, 10];
        let queue = taking_queue(&weights, &[0, 0, 0, 1], 2);
        let mut owner = TakeCursor::default();
        // The weighted file is heavy and goes first.
        assert_eq!(queue.next_fixture(0, &mut owner), Some((2, false)));
        // Lane 0 has no weight left; its files without weight are still there
        // for whoever is free.
        assert_eq!(drain(&queue, 1), [(3, false), (1, true), (0, true)]);
        assert_eq!(queue.next_fixture(0, &mut owner), None);
    }

    #[test]
    fn a_lane_admits_a_quarter_of_the_checkers_as_takers_from_eight_checkers_on() {
        // Eight checkers: two takers a lane at a time. Lane 0 holds all the
        // work; checkers 1 and 2 are admitted, checker 3 finds nothing
        // although work is left: its own checker and its takers finish it.
        let weights = vec![10; 40];
        let queue = taking_queue(&weights, &[0; 40], 8);
        assert_eq!(
            queue
                .taking
                .map(|taking| (taking.takers_per_lane, taking.sticky)),
            Some((2, false))
        );
        let (mut one, mut two, mut three) = Default::default();
        assert_eq!(queue.next_fixture(1, &mut one), Some((39, true)));
        assert_eq!(queue.next_fixture(2, &mut two), Some((38, true)));
        assert_eq!(queue.next_fixture(3, &mut three), None);
        // Checker 1 is still a taker of lane 0 and goes on.
        assert_eq!(queue.next_fixture(1, &mut one), Some((37, true)));
        // Seven and fewer, and sixteen and more: no limit.
        assert_eq!(
            taking_queue(&weights, &[0; 40], 7)
                .taking
                .map(|taking| taking.takers_per_lane),
            Some(0)
        );
        let sixteen = taking_queue(&weights, &[0; 40], 16);
        assert_eq!(
            sixteen
                .taking
                .map(|taking| (taking.takers_per_lane, taking.sticky)),
            Some((0, true))
        );
    }

    #[test]
    fn a_sticky_taker_stays_with_its_lane_while_that_has_half_the_fullest() {
        // Sixteen checkers. Lane 0 holds 6 x 10, lane 1 holds 5 x 10 + 1.
        // Checker 2 starts at lane 0 (60 against 51) and stays there while
        // lane 0 has at least half of what lane 1 has: 50, 40, 30 against 51
        // - at 20 it moves to lane 1. (A pool has no more lanes than files:
        // four files without weight on lane 3 make the sixteen; they are
        // taken last.)
        let mut weights = vec![10; 11];
        weights.push(1);
        weights.extend([0; 4]);
        let mut associations = vec![0; 6];
        associations.extend([1; 6]);
        associations.extend([3; 4]);
        let queue = taking_queue(&weights, &associations, 16);
        assert_eq!(queue.shard_count(), 16);
        assert_eq!(queue.taking.map(|taking| taking.sticky), Some(true));
        let taken = drain(&queue, 2)
            .into_iter()
            .map(|(file, _)| associations[file])
            .collect::<Vec<_>>();
        assert_eq!(taken[..4], [0, 0, 0, 0]);
        assert_eq!(taken[4], 1);
        assert_eq!(taken[12..], [3, 3, 3, 3]);
        assert_eq!(taken.len(), 16);
        // Without the sticky rule (four checkers) the taker alternates: the
        // fullest lane every time.
        let queue = taking_queue(&weights, &associations, 4);
        assert_eq!(queue.taking.map(|taking| taking.sticky), Some(false));
        let taken = drain(&queue, 2)
            .into_iter()
            .map(|(file, _)| associations[file])
            .collect::<Vec<_>>();
        assert_eq!(taken[..4], [0, 1, 0, 1]);
    }

    #[test]
    fn checkers_on_threads_check_every_file_once() {
        // Twelve checkers on threads over 3,000 files of uneven lanes: every
        // file is handed out exactly once, whoever takes it.
        let files = 3_000;
        let weights = (0..files).map(|file| 1 + file % 17).collect::<Vec<_>>();
        let associations = (0..files).map(|file| (file * file) % 5).collect::<Vec<_>>();
        let queue = taking_queue(&weights, &associations, 12);
        let seen = std::thread::scope(|scope| {
            let handles = (0..12)
                .map(|checker| {
                    let queue = &queue;
                    scope.spawn(move || drain(queue, checker))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .map(|(file, _)| file)
                .collect::<Vec<_>>()
        });
        let mut seen = seen;
        seen.sort_unstable();
        assert_eq!(seen, (0..files).collect::<Vec<_>>());
    }

    #[test]
    fn a_profile_levels_the_dealing_with_few_moves() {
        // Checker 0 holds 120 of 160 cost units; the mean is 53.
        let costs = [50, 30, 20, 10, 10, 20, 20];
        let mut associations = vec![0, 0, 0, 0, 0, 1, 2];
        let adjacent = vec![Vec::new(); 7];
        level_by_cost(&mut associations, &costs, &[true; 7], &adjacent, 3);
        // The budget is checker 0's 120 (it ends alone): checker 1, the
        // cheapest, may reach 40 and takes the 20; checker 2 may reach 60 and
        // takes the 30 and one 10. Three checkers at 60, 40 and 60: half the
        // longest load, and no more alive at any end than the budget holds.
        assert_eq!(associations, [0, 2, 1, 2, 0, 1, 2]);
        assert_eq!(capped_level(&[120, 20, 20]), (vec![120, 40, 60], 60));
        // A level dealing is left as it is, file for file.
        let mut level = vec![0, 1, 2, 0, 1, 2];
        level_by_cost(
            &mut level,
            &[10, 10, 10, 10, 10, 10],
            &[true; 6],
            &vec![Vec::new(); 6],
            3,
        );
        assert_eq!(level, [0, 1, 2, 0, 1, 2]);
        // A levelling that takes less than a twenty-fifth off the costliest
        // checker is not made: the 10 would go to checker 2 and leave 100 of
        // the 103.
        let mut slight = vec![0, 0, 1, 2];
        level_by_cost(
            &mut slight,
            &[93, 10, 100, 50],
            &[true; 4],
            &vec![Vec::new(); 4],
            3,
        );
        assert_eq!(slight, [0, 0, 1, 2]);
        // Checkers that end together already hold the whole budget: nothing
        // moves to the cheapest one.
        let mut together = vec![0, 0, 1, 2];
        level_by_cost(
            &mut together,
            &[100, 3, 100, 97],
            &[true; 4],
            &vec![Vec::new(); 4],
            3,
        );
        assert_eq!(together, [0, 0, 1, 2]);
        assert_eq!(capped_level(&[103, 100, 97]), (vec![291, 145, 97], 101));
        // Among the files that fit, the one whose import neighbours are on
        // the lightest checker goes; a file no checker type checks stays.
        let costs = [40, 10, 10, 10, 10];
        let mut associations = vec![0, 0, 0, 0, 1];
        let adjacent = vec![vec![], vec![], vec![4], vec![], vec![2]];
        level_by_cost(
            &mut associations,
            &costs,
            &[true, false, true, true, true],
            &adjacent,
            2,
        );
        assert_eq!(associations, [0, 0, 1, 1, 1]);
    }
}
