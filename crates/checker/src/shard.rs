//! tsgo's checker budget and deterministic weighted FENNEL association.
//!
//! A checker owns its caches and visits its associated files in Program order.
//! Neither worker scheduling nor file check durations can change ownership.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// tsgo `newCheckerPoolWithTracing`: max(min(requested, files, 256), 1).
pub const MAX_CHECKERS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckerBudget {
    checkers: NonZeroUsize,
    leak_states: bool,
    order_replay: bool,
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

    pub const fn checkers(self) -> usize {
        self.checkers.get()
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
