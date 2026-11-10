//! Exact outcomes of completed inference walks, owned by one checker.
//!
//! A fresh walk reads source/target, priority/contravariance and at most eight
//! infos' parameters, ordered candidate lists, priorities, top-level/fixed bits
//! and implied arities. It writes lists, priorities and top-level bits, and may
//! clear non-fixed cached inferred types. Absent lists/priorities are distinct.
//! Full keys are compared even when their table hashes collide.
//!
//! Only previously long targets are keyed. Storage requires 16 steps, no type,
//! symbol or signature creation, unchanged instantiation counters, no diagnostic
//! attempt or impure union reduction, no older transient/reference dependency or
//! expression-flow-cache access, no active mapper at entry, and no expression
//! check from a zero instantiation count. LS blocking excludes keys altogether.
//! Unlike tsrs's lazy-member variant, no symbol creation is exempted. All five
//! relation-cache sizes must also stay unchanged: they govern the TS2859 budget.
//!
//! These conditions exclude reads of unsettled resolution and deferred mapper
//! inputs. Remaining caches contain finished answers and type identities are
//! never recycled. A hit writes the exact outcome and replays flow height/flags.
//! COUNT_RESET is conservatively replayed whenever nested flow/frame work was
//! observed; excess reset flags only restrict the enclosing flow memo.
//! `infer-memo-shadow` executes each candidate hit from its original state and
//! asserts the outcome, effects, relation sizes and replay coverage. There is no
//! runtime switch and no verification/statistics code in ordinary builds.

use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashSet, FxHasher};
use tsc_types::{InferencePriority, RelationComparisonResult, TypeId};

use crate::flow_memo::{FLAG_COUNT_RESET, FLAG_EFFECTS, FLAG_TYPE_CACHE, UNTAINTED};
#[cfg(not(feature = "infer-memo-shadow"))]
use crate::inference::clear_cached_inferences;
use crate::inference::InferenceInfoId;
use crate::relate::RelationKind;
use crate::state::{CheckResult, CheckerState};

const MAX_INFOS: usize = 8;
const MIN_STEPS: u32 = 16;
const TABLE_SLOTS: usize = 1 << 12;
const MAX_LONG_TARGETS: usize = 1 << 16;
#[cfg(any(test, feature = "infer-memo-shadow"))]
const REJECTIONS: usize = 17;

#[derive(Clone, Debug, PartialEq, Eq)]
struct InfoOutcome {
    candidates: Option<Vec<TypeId>>,
    contra_candidates: Option<Vec<TypeId>>,
    priority: Option<InferencePriority>,
    top_level: bool,
}

#[derive(Clone)]
struct Entry {
    hash: u64,
    key: Box<[u32]>,
    infos: Box<[InfoOutcome]>,
    cleared: bool,
    height: u16,
    flags: u8,
    steps: u32,
}

#[cfg(any(test, feature = "infer-memo-shadow"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    walks: u64,
    lookups: u64,
    hits: u64,
    stores: u64,
    skipped_steps: u64,
    rejected: [u64; REJECTIONS],
    unkeyed: u64,
}

pub(crate) struct InferMemo {
    slots: Vec<Option<Entry>>,
    long_targets: FxHashSet<TypeId>,
    key_pool: Vec<Vec<u32>>,
    #[cfg(any(test, feature = "infer-memo-shadow"))]
    stats: Stats,
    #[cfg(feature = "infer-memo-shadow")]
    reported: std::cell::Cell<Stats>,
}

impl InferMemo {
    pub(crate) fn new() -> Self {
        Self {
            slots: Vec::new(),
            long_targets: FxHashSet::default(),
            key_pool: Vec::new(),
            #[cfg(any(test, feature = "infer-memo-shadow"))]
            stats: Stats::default(),
            #[cfg(feature = "infer-memo-shadow")]
            reported: std::cell::Cell::new(Stats::default()),
        }
    }

    fn lookup(&self, hash: u64, key: &[u32]) -> Option<&Entry> {
        let entry = self
            .slots
            .get(hash as usize & (TABLE_SLOTS - 1))?
            .as_ref()?;
        (entry.hash == hash && entry.key.as_ref() == key).then_some(entry)
    }

    fn store(&mut self, entry: Entry) {
        if self.slots.is_empty() {
            self.slots.resize_with(TABLE_SLOTS, || None);
        }
        let index = entry.hash as usize & (TABLE_SLOTS - 1);
        self.slots[index] = Some(entry);
        #[cfg(any(test, feature = "infer-memo-shadow"))]
        {
            self.stats.stores += 1;
        }
    }

    #[cfg(feature = "infer-memo-shadow")]
    pub(crate) fn report(&self, label: &str) {
        let now = self.stats;
        let before = self.reported.replace(now);
        if before == now {
            return;
        }
        let rejected: Vec<_> = now
            .rejected
            .iter()
            .zip(before.rejected)
            .map(|(a, b)| a - b)
            .collect();
        eprintln!("[infer-memo-shadow {label}] walks={} lookups={} hits={} stores={} skipped_steps={} unkeyed={} rejected={rejected:?}",
            now.walks - before.walks, now.lookups - before.lookups, now.hits - before.hits,
            now.stores - before.stores, now.skipped_steps - before.skipped_steps, now.unkeyed - before.unkeyed);
    }
}

#[cfg(feature = "infer-memo-shadow")]
impl Drop for InferMemo {
    fn drop(&mut self) {
        self.report("final");
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Effects {
    created: (usize, usize, usize),
    local: u64,
    total: u64,
    diagnostics: (u64, usize, usize, usize),
    impure: u64,
    relations: [usize; 5],
    reliability: RelationComparisonResult,
    flow_disabled: bool,
    flow_invocations: u32,
}

fn key_hash(key: &[u32]) -> u64 {
    let mut hash = FxHasher::default();
    key.hash(&mut hash);
    hash.finish()
}

impl CheckerState<'_> {
    fn infer_memo_effects(&self) -> Effects {
        Effects {
            created: (
                self.tables.len(),
                self.binder.transient_symbol_count(),
                self.signatures.len(),
            ),
            local: self.instantiation_count,
            total: self.total_instantiation_count,
            diagnostics: (
                self.diagnostics.attempts,
                self.diagnostics.len(),
                self.deferred_diagnostics.len(),
                self.visible_global_diagnostics.len(),
            ),
            impure: self.impure_union_reductions,
            relations: RelationKind::ALL.map(|r| self.relations.cache(r).len()),
            reliability: self.reliability_flags,
            flow_disabled: self.flow_analysis_disabled,
            flow_invocations: self.flow_invocation_count,
        }
    }

    fn infer_memo_key(
        &self,
        key: &mut Vec<u32>,
        infos: &[InferenceInfoId],
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
        contravariant: bool,
    ) -> bool {
        if infos.len() > MAX_INFOS || !self.skip_direct_inference_nodes.is_empty() {
            return false;
        }
        key.clear();
        key.extend([
            source.index(),
            target.index(),
            priority.bits() as u32,
            u32::from(contravariant),
            infos.len() as u32,
        ]);
        for (position, &id) in infos.iter().enumerate() {
            let info = self.inference_info(id);
            // Repeated parameters could alias the same info object in one call
            // and distinct objects in another. The specified value key would
            // not describe that aliasing, so such calls use the real walk.
            if infos[..position]
                .iter()
                .any(|&other| self.inference_info(other).type_parameter == info.type_parameter)
            {
                return false;
            }
            key.extend([
                info.type_parameter.index(),
                u32::from(info.priority.is_some()),
                info.priority.unwrap_or_default().bits() as u32,
                u32::from(info.top_level) | (u32::from(info.is_fixed) << 1),
                u32::from(info.implied_arity.is_some()),
            ]);
            let arity = info.implied_arity.unwrap_or_default() as u64;
            key.extend([arity as u32, (arity >> 32) as u32]);
            for list in [&info.candidates, &info.contra_candidates] {
                key.push(u32::from(list.is_some()));
                if let Some(list) = list {
                    key.push(
                        u32::try_from(list.len()).expect("candidate list fits type identity space"),
                    );
                    key.extend(list.iter().map(|t| t.index()));
                }
            }
        }
        true
    }

    fn infer_memo_outcomes(&self, infos: &[InferenceInfoId]) -> Box<[InfoOutcome]> {
        infos
            .iter()
            .map(|&id| {
                let info = self.inference_info(id);
                InfoOutcome {
                    candidates: info.candidates.clone(),
                    contra_candidates: info.contra_candidates.clone(),
                    priority: info.priority,
                    top_level: info.top_level,
                }
            })
            .collect()
    }

    /// The existing inferTypes operation, with exact reuse of pure long walks.
    pub(crate) fn infer_types(
        &mut self,
        infos: &[InferenceInfoId],
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
        contravariant: bool,
    ) -> CheckResult<()> {
        #[cfg(any(test, feature = "infer-memo-shadow"))]
        {
            self.infer_memo.stats.walks += 1;
        }
        if !self.infer_memo.long_targets.contains(&target) {
            let (result, steps, _) =
                self.infer_types_walk(infos, source, target, priority, contravariant);
            if steps >= MIN_STEPS && self.infer_memo.long_targets.len() < MAX_LONG_TARGETS {
                self.infer_memo.long_targets.insert(target);
            }
            return result;
        }
        let mut key = self.infer_memo.key_pool.pop().unwrap_or_default();
        if !self.infer_memo_key(&mut key, infos, source, target, priority, contravariant) {
            self.infer_memo.key_pool.push(key);
            #[cfg(any(test, feature = "infer-memo-shadow"))]
            {
                self.infer_memo.stats.unkeyed += 1;
            }
            return self
                .infer_types_walk(infos, source, target, priority, contravariant)
                .0;
        }
        let hash = key_hash(&key);
        #[cfg(any(test, feature = "infer-memo-shadow"))]
        {
            self.infer_memo.stats.lookups += 1;
        }
        if let Some(entry) = self.infer_memo.lookup(hash, &key) {
            let (height, flags, steps) = (entry.height, entry.flags, entry.steps);
            #[cfg(feature = "infer-memo-shadow")]
            {
                let entry = entry.clone();
                self.infer_memo_shadow(infos, source, target, priority, contravariant, &entry);
            }
            #[cfg(not(feature = "infer-memo-shadow"))]
            {
                for (&id, outcome) in infos.iter().zip(entry.infos.iter()) {
                    let info = &mut self.inference_info_arena[id.index() as usize];
                    info.candidates.clone_from(&outcome.candidates);
                    info.contra_candidates
                        .clone_from(&outcome.contra_candidates);
                    info.priority = outcome.priority;
                    info.top_level = outcome.top_level;
                }
                if entry.cleared {
                    clear_cached_inferences(&mut self.inference_info_arena, infos);
                }
            }
            #[cfg(any(test, feature = "infer-memo-shadow"))]
            {
                self.infer_memo.stats.hits += 1;
                self.infer_memo.stats.skipped_steps += u64::from(steps);
            }
            #[cfg(not(any(test, feature = "infer-memo-shadow")))]
            let _ = steps;
            self.infer_memo.key_pool.push(key);
            self.flow_memo.raise_height(height);
            self.flow_memo.add_flags(flags);
            return Ok(());
        }
        let before = self.infer_memo_effects();
        let expressions = self.expression_checks;
        let active = !self.active_type_mappers.is_empty();
        // A circular variance query can temporarily publish an empty list
        // while an older measurement will later replace it (variance.rs).
        let in_variance = !self.variance_stack.is_empty();
        self.flow_frame_begin();
        let serial = self.flow_memo.source_now();
        let (result, steps, cleared) =
            self.infer_types_walk(infos, source, target, priority, contravariant);
        let (taint, height, flags) = self.flow_memo.end_transparent();
        let after = self.infer_memo_effects();
        let rejected = [
            steps < MIN_STEPS,
            active,
            before.created != after.created,
            before.local != after.local,
            before.total != after.total,
            before.diagnostics != after.diagnostics,
            before.impure != after.impure,
            taint.transient != UNTAINTED,
            taint.reference != UNTAINTED,
            flags & FLAG_EFFECTS != 0,
            flags & FLAG_TYPE_CACHE != 0,
            before.local == 0 && self.expression_checks != expressions,
            !self.skip_direct_inference_nodes.is_empty(),
            before.relations != after.relations,
            before.reliability != after.reliability
                || before.flow_disabled != after.flow_disabled
                || result.is_err(),
            // get_type_of_expression decides whether to publish a flow-cache
            // answer from this counter's change (expr.rs). A hit must not
            // suppress that publication by omitting an invocation increment.
            before.flow_invocations != after.flow_invocations,
            in_variance,
        ];
        if rejected.iter().any(|&r| r) {
            #[cfg(any(test, feature = "infer-memo-shadow"))]
            for (count, rejected) in self.infer_memo.stats.rejected.iter_mut().zip(rejected) {
                *count += u64::from(rejected);
            }
            self.infer_memo.key_pool.push(key);
            return result;
        }
        let flags = flags
            | if self.flow_memo.source_now() != serial {
                FLAG_COUNT_RESET
            } else {
                0
            };
        let entry = Entry {
            hash,
            key: key.into_boxed_slice(),
            infos: self.infer_memo_outcomes(infos),
            cleared,
            height,
            flags,
            steps,
        };
        self.infer_memo.store(entry);
        result
    }

    #[cfg(feature = "infer-memo-shadow")]
    fn infer_memo_shadow(
        &mut self,
        infos: &[InferenceInfoId],
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
        contravariant: bool,
        entry: &Entry,
    ) {
        let before = self.infer_memo_effects();
        let expressions = self.expression_checks;
        self.flow_frame_begin();
        let (result, _, cleared) =
            self.infer_types_walk(infos, source, target, priority, contravariant);
        let (taint, height, flags) = self.flow_memo.end_transparent();
        let after = self.infer_memo_effects();
        let outcome = self.infer_memo_outcomes(infos);
        assert!(result.is_ok() && before == after
            && (before.local != 0 || expressions == self.expression_checks)
            && taint.transient == UNTAINTED && taint.reference == UNTAINTED
            && outcome == entry.infos && cleared == entry.cleared
            && height <= entry.height && flags & !entry.flags == 0,
            "infer-memo-shadow: source={source:?} target={target:?} priority={priority:?} before={before:?} after={after:?} outcome={outcome:?} stored={:?} cleared={cleared}/{} taint={taint:?} height={height}/{} flags={flags}/{}",
            entry.infos, entry.cleared, entry.height, entry.flags);
    }
}

#[cfg(test)]
#[path = "../tests/unit/infer_memo/tests.rs"]
mod tests;
