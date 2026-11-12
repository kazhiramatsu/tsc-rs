//! tsgo's checker budget and deterministic weighted FENNEL association, and
//! the fast preset's association by directory locality.
//!
//! A checker owns its caches and visits its associated files in Program order.
//! Neither worker scheduling nor file check durations can change ownership:
//! under either association the owner of a file is a function of the Program.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// The fast preset's pool: one checker per this many type-checked files,
/// between four (tsgo's default) and twelve. The count follows the Program
/// and never the machine, so the preset's output is the same on every machine.
/// Every checker resolves again some of what the others resolved, so the work
/// grows with the pool (about 0.6 % per checker on a ten-thousand-file
/// program); twelve was the fastest pool on ten cores and still divides the
/// work finely enough that four cores share it evenly.
const FAST_FILES_PER_CHECKER: usize = 32;
const FAST_MIN_CHECKERS: usize = 4;
const FAST_MAX_CHECKERS: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckerBudget {
    checkers: NonZeroUsize,
    leak_states: bool,
    order_replay: bool,
    assignment: CheckerAssignment,
    /// The count is an upper bound that follows the Program's checked files.
    scaled: bool,
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

    /// The fast preset without an explicit count: the locality association
    /// and a pool that follows the Program's checked files.
    pub fn fast() -> Self {
        Self {
            scaled: true,
            ..Self::new(NonZeroUsize::new(FAST_MAX_CHECKERS).expect("twelve is nonzero"))
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

/// What a checker pays to hold a module: it resolves the part of the module
/// its files use, which grows slower than the module.
fn module_worth(nodes: usize) -> f64 {
    (nodes as f64).sqrt()
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
    checkers: usize,
) -> Vec<usize> {
    let checkers = checkers.clamp(1, base_weights.len().clamp(1, MAX_CHECKERS));
    let mut associations = vec![0; base_weights.len()];
    if checkers == 1 {
        return associations;
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
        return associations;
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
        }
        group_of[file] = group;
        group_weights[group] += weights[file];
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
    let worth = nodes
        .iter()
        .map(|&nodes| module_worth(nodes))
        .collect::<Vec<_>>();
    let placed = place_groups(
        &group_weights,
        &group_adjacent,
        &group_modules,
        &worth,
        checkers,
    );
    for &file in &order {
        associations[file] = placed[group_of[file]];
    }
    associations
}

/// Steps 3 and 4 of [`associate_files_by_locality`]: tsgo's FENNEL step over
/// the groups with the module term, then the refinement.
fn place_groups(
    weights: &[usize],
    adjacent: &[Vec<usize>],
    modules: &[Vec<usize>],
    worth: &[f64],
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
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for (checker, &load) in loads.iter().enumerate() {
            if load + weights[group] > cap {
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
            (0..checkers).min_by_key(|&checker| loads[checker]).unwrap()
        });
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

/// Each lane has a fixed owner. Cursors are independent; no lane steals or
/// pulls another lane's work. Empty checkers are retained, as in tsgo's pool.
#[derive(Debug)]
pub(crate) struct ShardFileQueue {
    lanes: Vec<Lane>,
    aborted: AtomicBool,
}

#[derive(Debug, Default)]
struct Lane {
    fixtures: Vec<usize>,
    libraries: Vec<usize>,
    weight: usize,
    next_fixture: AtomicUsize,
    next_library: AtomicUsize,
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
        Self {
            lanes,
            aborted: AtomicBool::new(false),
        }
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

    pub(crate) fn next_fixture(&self, shard: usize) -> Option<usize> {
        if self.aborted() {
            return None;
        }
        let lane = &self.lanes[shard];
        lane.fixtures
            .get(lane.next_fixture.fetch_add(1, Ordering::Relaxed))
            .copied()
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
        let fast = CheckerBudget::fast();
        assert_eq!(fast.assignment(), CheckerAssignment::Locality);
        assert_eq!(
            CheckerBudget::automatic().assignment(),
            CheckerAssignment::Tsgo
        );
        // One checker per 32 checked files, between 4 and 12, never more
        // than files; nothing here reads the machine.
        assert_eq!(fast.checkers_for(10_270, 9_243), 12);
        assert_eq!(fast.checkers_for(739, 440), 12);
        assert_eq!(fast.checkers_for(600, 352), 11);
        assert_eq!(fast.checkers_for(362, 187), 5);
        assert_eq!(fast.checkers_for(249, 78), 4);
        assert_eq!(fast.checkers_for(3, 3), 3);
        assert_eq!(fast.checkers_for(0, 0), 1);
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
            associate_files_by_locality(
                &self.names,
                &self.base,
                &self.imports,
                &self.checked,
                &self.adjacent,
                &self.targets,
                &self.base,
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
        assert_eq!(program.associate(3), [2, 2, 2, 2, 2, 0, 1, 0, 2, 1]);
        assert_eq!(program.associate(4), [2, 2, 2, 2, 2, 0, 1, 0, 2, 1]);
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
        assert_eq!(program.associate(3), [2, 2, 2, 2, 0, 1, 2, 0, 0, 0, 0, 1]);
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
        assert!(associate_files_by_locality(&[], &[], &[], &[], &[], &[], &[], 4).is_empty());
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
                while let Some(file) = queue.next_fixture(checker) {
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
}
