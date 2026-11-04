//! Bounded, per-checker flow memo and dependency registers (tsrs C-0).
//! The default build compiles this code but never allocates or consults the
//! table. `flow-memo-shadow` fills it and validates hits with the real walk.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};
use tsc_binder::flow::FlowId;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{FlowFlags, NodeFlags, TypeId};

use crate::flow::{FlowQuery, FlowType, SharedFlow};
use crate::state::CheckerState;

pub(crate) const FLOW_DEPTH_LIMIT: u32 = 2000;
const TABLE_SLOTS: usize = 1 << 14;
/// Step 3 owns enabling replacement. There is no runtime mode switch.
const MEMO_ENABLED: bool = true;
pub(crate) const MEMO_TRACKED: bool = MEMO_ENABLED || cfg!(feature = "flow-memo-shadow");
pub(crate) const UNTAINTED: u32 = u32::MAX;
/// Instantiated, began under an active mapper, or created an identity.
pub(crate) const FLAG_EFFECTS: u8 = 1;
/// Read the expression flow cache; the answer depends on its epoch.
pub(crate) const FLAG_TYPE_CACHE: u8 = 2;
/// May have reset the per-entry instantiation count.
pub(crate) const FLAG_COUNT_RESET: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Regs {
    transient: u32,
    reference: u32,
    height: u16,
    flags: u8,
}

const FRESH_REGS: Regs = Regs {
    transient: UNTAINTED,
    reference: UNTAINTED,
    height: 0,
    flags: 0,
};

#[derive(Clone, Copy, Default)]
struct FlowCounters {
    instantiation_count: u64,
    total_instantiation_count: u64,
    created: usize,
    in_instantiation: bool,
}

struct FlowFrame {
    start: u32,
    saved: Regs,
    counters: FlowCounters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTaint {
    pub(crate) transient: u32,
    pub(crate) reference: u32,
    pub(crate) flags: u8,
}

#[derive(Clone)]
pub(crate) struct MemoKey {
    hash: u128,
    full: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MemoHit {
    ty: TypeId,
    height: u16,
    epoch: u32,
    flags: u8,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    key: u128,
    ty: Option<TypeId>,
    epoch: u32,
    height: u16,
    flags: u8,
}

const _: () = assert!(std::mem::size_of::<Slot>() == 32);

#[derive(Clone)]
struct ShadowOrigin {
    key: Arc<[u8]>,
    file: usize,
    flow: FlowId,
    reference: NodeId,
}

struct ShadowHit {
    flow: FlowId,
    hit: MemoHit,
    origin: ShadowOrigin,
    before: FrameTaint,
    height_before: u16,
    shared_start: usize,
    shared_hash: u64,
    ends_iteration: bool,
    shared_node: Option<FlowId>,
}

pub(crate) struct FrameMemo {
    key: MemoKey,
    checkpoints: usize,
    shadow: usize,
    shared_start: usize,
}

#[derive(Default)]
struct MemoStats {
    lookups: u64,
    hits: u64,
    stores: u64,
    disagreements: u64,
    occupied: usize,
}

pub(crate) struct FlowMemo {
    serial: u32,
    regs: Regs,
    frames: Vec<FlowFrame>,
    /// Saved/restored with flow_type_cache; an entry replacement gets a
    /// fresh epoch too. UNTAINTED identifies the initial, empty cache.
    pub(crate) type_cache_epoch: u32,
    slots: Vec<Slot>,
    shadow_slots: Vec<Option<ShadowOrigin>>,
    checkpoints: Vec<FlowId>,
    shadow_hits: Vec<ShadowHit>,
    stats: MemoStats,
    #[cfg(feature = "flow-memo-shadow")]
    reported: std::cell::Cell<(u64, u64, u64, u64)>,
}

impl FlowMemo {
    pub(crate) fn new() -> Self {
        Self {
            serial: 1,
            regs: FRESH_REGS,
            frames: Vec::new(),
            type_cache_epoch: UNTAINTED,
            slots: Vec::new(),
            shadow_slots: Vec::new(),
            checkpoints: Vec::new(),
            shadow_hits: Vec::new(),
            stats: MemoStats::default(),
            #[cfg(feature = "flow-memo-shadow")]
            reported: std::cell::Cell::new((0, 0, 0, 0)),
        }
    }

    /// A source starting now need not take a unique serial: every frame
    /// begun later will have a larger start.
    #[inline]
    pub(crate) fn source_now(&self) -> u32 {
        self.serial - 1
    }

    #[inline]
    pub(crate) fn next_serial(&mut self) -> u32 {
        let serial = self.serial;
        // Never wrap or return UNTAINTED as a source. After exhaustion,
        // all existing and future frames are permanently tainted.
        self.serial = self.serial.saturating_add(1).min(UNTAINTED - 1);
        if self.serial == UNTAINTED - 1 {
            self.taint(0);
        }
        serial
    }

    #[inline]
    fn begin(&mut self, counters: FlowCounters) {
        let start = self.next_serial();
        self.frames.push(FlowFrame {
            start,
            saved: self.regs,
            counters,
        });
        self.regs = FRESH_REGS;
        if self.serial == UNTAINTED - 1 {
            self.taint(0);
        }
    }

    /// Transparent frames (C-1) propagate children without adding a flow
    /// level. Dependencies born inside a frame do not taint its answer,
    /// but their serials still propagate to older enclosing frames.
    #[inline]
    pub(crate) fn end_transparent(&mut self) -> (FrameTaint, u16, u8) {
        let frame = self
            .frames
            .pop()
            .expect("flow frame without matching begin");
        let r = self.regs;
        let taint = FrameTaint {
            transient: if r.transient < frame.start {
                r.transient
            } else {
                UNTAINTED
            },
            reference: if r.reference < frame.start {
                r.reference
            } else {
                UNTAINTED
            },
            flags: r.flags,
        };
        self.regs = Regs {
            transient: r.transient.min(frame.saved.transient),
            reference: r.reference.min(frame.saved.reference),
            height: r.height.max(frame.saved.height),
            flags: r.flags | frame.saved.flags,
        };
        (taint, r.height, r.flags)
    }

    #[inline]
    fn end(&mut self, counters: FlowCounters) -> (FrameTaint, u16) {
        let before = self
            .frames
            .last()
            .expect("flow frame without matching begin")
            .counters;
        self.add_flags(counter_flags(before, counters));
        let (taint, height, _) = self.end_transparent();
        self.raise_height(height.saturating_add(1));
        (taint, height)
    }

    #[inline]
    pub(crate) fn raise_height(&mut self, height: u16) {
        self.regs.height = self.regs.height.max(height);
    }

    #[inline]
    pub(crate) fn add_flags(&mut self, flags: u8) {
        self.regs.flags |= flags;
    }

    #[inline]
    pub(crate) fn taint(&mut self, source: u32) {
        self.regs.transient = self.regs.transient.min(source);
    }

    #[inline]
    pub(crate) fn taint_reference(&mut self, source: u32) {
        self.regs.reference = self.regs.reference.min(source);
    }

    #[inline]
    pub(crate) fn save_height(&mut self) -> u16 {
        std::mem::take(&mut self.regs.height)
    }

    #[inline]
    pub(crate) fn restore_height(&mut self, saved: u16) {
        self.regs.height = saved;
    }

    pub(crate) fn frame_depth(&self) -> usize {
        self.frames.len()
    }

    fn frame_taint(&self, counters: FlowCounters) -> FrameTaint {
        let frame = self.frames.last().expect("active flow frame");
        FrameTaint {
            transient: if self.regs.transient < frame.start {
                self.regs.transient
            } else {
                UNTAINTED
            },
            reference: if self.regs.reference < frame.start {
                self.regs.reference
            } else {
                UNTAINTED
            },
            flags: self.regs.flags | counter_flags(frame.counters, counters),
        }
    }

    fn node_key(file: usize, flow: FlowId, key: &MemoKey) -> u128 {
        hash_key(&(file, flow, key.hash))
    }

    fn slot_index(key: u128) -> usize {
        ((key as u64) ^ ((key >> 64) as u64).rotate_left(29)) as usize & (TABLE_SLOTS - 1)
    }

    fn lookup(&mut self, file: usize, flow: FlowId, key: &MemoKey) -> Option<MemoHit> {
        self.stats.lookups += 1;
        let key = Self::node_key(file, flow, key);
        let slot = self.slots.get(Self::slot_index(key))?;
        if slot.key != key {
            return None;
        }
        Some(MemoHit {
            ty: slot.ty?,
            height: slot.height,
            epoch: slot.epoch,
            flags: slot.flags,
        })
    }

    fn store(&mut self, query: &FlowQuery, flow: FlowId, key: &MemoKey, hit: MemoHit) {
        if self.slots.is_empty() {
            self.slots.resize(TABLE_SLOTS, Slot::default());
            if cfg!(feature = "flow-memo-shadow") {
                self.shadow_slots.resize(TABLE_SLOTS, None);
            }
        }
        let node_key = Self::node_key(query.file, flow, key);
        let index = Self::slot_index(node_key);
        self.stats.occupied += usize::from(self.slots[index].ty.is_none());
        self.slots[index] = Slot {
            key: node_key,
            ty: Some(hit.ty),
            epoch: hit.epoch,
            height: hit.height,
            flags: hit.flags,
        };
        self.stats.stores += 1;
        if cfg!(feature = "flow-memo-shadow") {
            self.shadow_slots[index] = Some(ShadowOrigin {
                key: Arc::clone(&key.full),
                file: query.file,
                flow,
                reference: query.reference,
            });
        }
    }

    /// Uses the existing counter sidecar's name and name=value format.
    /// A companion file avoids racing the compiler's final counter write.
    #[cfg(feature = "flow-memo-shadow")]
    pub(crate) fn report(&self, label: &str) {
        use std::io::Write;
        let s = &self.stats;
        let now = (s.lookups, s.hits, s.stores, s.disagreements);
        let before = self.reported.replace(now);
        if before == now {
            return;
        }
        let key_bytes: usize = self
            .shadow_slots
            .iter()
            .flatten()
            .map(|s| s.key.len())
            .sum();
        let report = format!(
            "flow_memo.lookups={}\nflow_memo.hits={}\nflow_memo.stores={}\nflow_memo.disagreements={}\nflow_memo.entries={}\nflow_memo.slots={}\nflow_memo.table_bytes={}\nflow_memo.shadow_slot_bytes={}\nflow_memo.shadow_key_bytes={}\n",
            now.0 - before.0, now.1 - before.1, now.2 - before.2, now.3 - before.3, s.occupied,
            self.slots.len(), self.slots.len() * std::mem::size_of::<Slot>(),
            self.shadow_slots.len() * std::mem::size_of::<Option<ShadowOrigin>>(), key_bytes,
        );
        if let Some(mut path) = std::env::var_os("TSRS_PERF_COUNTERS") {
            path.push(".flow-memo");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(report.as_bytes());
            }
        } else {
            eprintln!("[flow-memo-shadow {label}] {}", report.replace('\n', " "));
        }
    }
}

#[cfg(feature = "flow-memo-shadow")]
impl Drop for FlowMemo {
    fn drop(&mut self) {
        // Harness type/symbol walks may query a retained checker after the
        // check-phase report. Report those remaining counters exactly once.
        self.report("final");
    }
}

fn counter_flags(before: FlowCounters, after: FlowCounters) -> u8 {
    let effects = before.total_instantiation_count != after.total_instantiation_count
        || before.in_instantiation
        || before.created != after.created;
    let reset =
        before.instantiation_count == 0 || before.instantiation_count != after.instantiation_count;
    (u8::from(effects) * FLAG_EFFECTS) | (u8::from(reset) * FLAG_COUNT_RESET)
}

/// Two domain-separated hashes; shadow mode retains and compares the full key.
fn hash_key(value: &impl Hash) -> u128 {
    let mut low = FxHasher::default();
    let mut high = FxHasher::default();
    0x9e37_79b9_7f4a_7c15_u64.hash(&mut low);
    0xff51_afd7_ed55_8ccd_u64.hash(&mut high);
    value.hash(&mut low);
    value.hash(&mut high);
    u128::from(low.finish()) | (u128::from(high.finish()) << 64)
}

fn is_checkpoint(file: usize, flow: FlowId) -> bool {
    // Stable for a node regardless of where the current iteration started.
    hash_key(&(file, flow)) & 3 == 0
}

pub(crate) fn memo_shared_node(ends_iteration: bool, shared: Option<FlowId>) -> Option<FlowId> {
    if ends_iteration {
        shared
    } else {
        None
    }
}

fn can_store(taint: FrameTaint, ty: FlowType, height: u16, steps: u32, disabled: bool) -> bool {
    taint.transient == UNTAINTED
        && taint.reference == UNTAINTED
        && taint.flags & FLAG_EFFECTS == 0
        && !ty.is_incomplete()
        && !disabled
        && u32::from(height) < FLOW_DEPTH_LIMIT
        && (height != 0 || steps >= 4)
}

/// Pure shared answers may safely be omitted by a memo hit. Transient or
/// effectful additions can change what a later part of this walk consumes.
fn shared_replay_safe(shared: &[SharedFlow]) -> bool {
    shared.iter().all(|s| {
        s.transient == UNTAINTED
            && s.reference == UNTAINTED
            && s.flags & FLAG_EFFECTS == 0
            && !s.flow_type.is_incomplete()
    })
}

fn shared_hash(shared: &[SharedFlow]) -> u64 {
    let mut hash = FxHasher::default();
    for s in shared {
        (
            s.flow,
            s.flow_type.get_type(),
            s.flow_type.is_incomplete(),
            s.transient,
            s.reference,
            s.height,
            s.flags,
        )
            .hash(&mut hash);
    }
    hash.finish()
}

pub(crate) struct SavedFlowTypeCache {
    cache: Option<FxHashMap<NodeId, TypeId>>,
    epoch: u32,
}

impl CheckerState<'_> {
    /// Only reads cached links and syntax. In particular, never resolves a
    /// symbol or assigns a node/symbol identity while constructing a key.
    pub(crate) fn flow_memo_key(&self, query: &FlowQuery) -> Option<MemoKey> {
        if query.synthetic_props.is_some() || query.synthetic_this_root {
            return None;
        }
        let mut bytes = Vec::new();
        self.write_flow_memo_reference(query.reference, &mut bytes)?;
        bytes.extend_from_slice(&query.declared_type.index().to_le_bytes());
        bytes.extend_from_slice(&query.initial_type.index().to_le_bytes());
        bytes.extend_from_slice(
            &query
                .flow_container
                .map_or(0, NodeId::position)
                .to_le_bytes(),
        );
        // Like tsrs, inline/reduce modes gate frames rather than being key
        // fields. Synthetic modes above have no structural reference key.
        Some(MemoKey {
            hash: hash_key(&bytes),
            full: bytes.into(),
        })
    }

    fn write_flow_memo_reference(&self, node: NodeId, bytes: &mut Vec<u8>) -> Option<()> {
        match self.data_of(node) {
            NodeData::Identifier(_) => {
                let symbol = self.links.node(node).resolved_symbol.resolved()?;
                if symbol == self.unknown_symbol || self.is_this_in_type_query(node) {
                    return None;
                }
                bytes.push(b'I');
                bytes.extend_from_slice(&symbol.position().to_le_bytes());
            }
            NodeData::PropertyAccessExpression(data) => {
                if self
                    .binder
                    .flags_of(node)
                    .intersects(NodeFlags::OPTIONAL_CHAIN)
                {
                    return None;
                }
                self.write_flow_memo_reference(data.expression?, bytes)?;
                let name = self.escaped_text_of(data.name)?;
                bytes.push(b'P');
                bytes.extend_from_slice(&u32::try_from(name.len()).ok()?.to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
            }
            _ if self.kind_of(node) == SyntaxKind::ThisKeyword => bytes.push(b'T'),
            _ => return None,
        }
        Some(())
    }

    pub(crate) fn flow_frame_memo(&self, query: &FlowQuery) -> Option<FrameMemo> {
        if !MEMO_TRACKED
            || self.inline_level != 0
            || query.reduce_depth != 0
            || !self.reduce_label_overrides.is_empty()
            || self.flow_analysis_disabled
        {
            return None;
        }
        Some(FrameMemo {
            key: query.memo_key.clone()?,
            checkpoints: self.flow_memo.checkpoints.len(),
            shadow: self.flow_memo.shadow_hits.len(),
            shared_start: self.shared_flow.len(),
        })
    }

    fn flow_memo_consult_ok(&self, query: &FlowQuery, hit: MemoHit) -> bool {
        !query.memo_faithful
            && !self.flow_analysis_disabled
            && self.reduce_label_overrides.is_empty()
            && self.flow_loop_stack.is_empty()
            && (hit.epoch == 0 || hit.epoch == self.flow_memo.type_cache_epoch)
            && self.active_type_mappers.is_empty()
            && (hit.flags & FLAG_COUNT_RESET == 0 || self.instantiation_count == 0)
            && query.impure_shared == 0
            && query.flow_depth - 1 + u32::from(hit.height) < FLOW_DEPTH_LIMIT
            && self.flow_memo.serial < UNTAINTED - 1
    }

    /// Returns an answer only for a future enabled non-shadow build. In
    /// shadow builds each candidate is kept until its real frame finishes.
    pub(crate) fn flow_memo_at_node(
        &mut self,
        query: &FlowQuery,
        memo: &FrameMemo,
        entry: FlowId,
        flow: FlowId,
        flags: FlowFlags,
        shared: Option<FlowId>,
    ) -> Option<(FlowType, bool)> {
        let ends_iteration = flags.intersects(
            FlowFlags::TRUE_CONDITION | FlowFlags::FALSE_CONDITION | FlowFlags::SWITCH_CLAUSE,
        ) || flags.intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL)
            && self.flow_label_antecedents(query.file, flow).len() > 1;
        if flow != entry && !ends_iteration {
            if !is_checkpoint(query.file, flow) {
                return None;
            }
            self.flow_memo.checkpoints.push(flow);
        }
        let hit = self.flow_memo.lookup(query.file, flow, &memo.key)?;
        if !self.flow_memo_consult_ok(query, hit) {
            return None;
        }
        self.flow_memo.stats.hits += 1;
        if cfg!(feature = "flow-memo-shadow") {
            let index = FlowMemo::slot_index(FlowMemo::node_key(query.file, flow, &memo.key));
            let origin = self.flow_memo.shadow_slots[index]
                .as_ref()
                .expect("shadow slot")
                .clone();
            self.flow_memo.shadow_hits.push(ShadowHit {
                flow,
                hit,
                origin,
                before: self.flow_memo.frame_taint(self.flow_frame_counters()),
                height_before: self.flow_memo.regs.height,
                shared_start: self.shared_flow.len(),
                shared_hash: shared_hash(&self.shared_flow[query.shared_flow_start..]),
                ends_iteration,
                shared_node: memo_shared_node(ends_iteration, shared),
            });
            return None;
        }
        self.flow_memo.raise_height(hit.height);
        self.flow_memo.add_flags(hit.flags);
        Some((FlowType::Type(hit.ty), ends_iteration))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn flow_memo_finish(
        &mut self,
        query: &FlowQuery,
        memo: FrameMemo,
        entry: FlowId,
        final_flow: FlowId,
        result: &crate::state::CheckResult<FlowType>,
        taint: FrameTaint,
        height: u16,
        steps: u32,
        shared: Option<FlowId>,
        shared_hit: bool,
    ) {
        // Settle BEFORE filling: nested frames can replace any direct-mapped
        // slot, so each pending hit owns the answer and full key it found.
        while self.flow_memo.shadow_hits.len() > memo.shadow {
            let shadow = self
                .flow_memo
                .shadow_hits
                .pop()
                .expect("pending shadow hit");
            self.flow_memo_shadow_check(query, &memo.key, result, taint, height, shadow, shared);
        }
        // A shared-flow hit settles pending shadow hits but is not a fresh
        // sub-walk. In particular its cache flags describe its original
        // computation, not necessarily the expression-cache epoch now.
        let store = !shared_hit
            && result.as_ref().is_ok_and(|&ty| {
                can_store(taint, ty, height, steps, self.flow_analysis_disabled)
                    && !query.memo_aborted
                    && shared_replay_safe(&self.shared_flow[memo.shared_start..])
            });
        if store {
            let hit = MemoHit {
                ty: result.as_ref().expect("successful flow result").get_type(),
                height,
                // A cold computation can be replayed through a warm
                // expression-cache read. Conservatively bind every answer
                // to this epoch and carry that dependency on replacement.
                // This tightens consultation without relaxing the shadow
                // check of the real walk's registers.
                epoch: self.flow_memo.type_cache_epoch,
                flags: taint.flags | FLAG_TYPE_CACHE,
            };
            self.flow_memo.store(query, entry, &memo.key, hit);
            if final_flow != entry {
                self.flow_memo.store(query, final_flow, &memo.key, hit);
            }
            for i in memo.checkpoints..self.flow_memo.checkpoints.len() {
                let checkpoint = self.flow_memo.checkpoints[i];
                if checkpoint != final_flow {
                    self.flow_memo.store(query, checkpoint, &memo.key, hit);
                }
            }
        }
        self.flow_memo.checkpoints.truncate(memo.checkpoints);
    }

    #[allow(clippy::too_many_arguments)]
    fn flow_memo_shadow_check(
        &mut self,
        query: &FlowQuery,
        key: &MemoKey,
        result: &crate::state::CheckResult<FlowType>,
        taint: FrameTaint,
        height: u16,
        shadow: ShadowHit,
        shared: Option<FlowId>,
    ) {
        let same_key = shadow.origin.key == key.full
            && shadow.origin.file == query.file
            && shadow.origin.flow == shadow.flow;
        let same_type = result
            .as_ref()
            .is_ok_and(|&t| t == FlowType::Type(shadow.hit.ty));
        // Heights and flags are upper bounds: warm caches may remove work.
        // Compare dependencies relative to the frame, including any prefix
        // already walked before this checkpoint's hypothetical replacement.
        let registers_ok = taint.transient == shadow.before.transient
            && taint.reference == shadow.before.reference
            && taint.flags & !(shadow.before.flags | shadow.hit.flags) == 0
            && height <= shadow.height_before.max(shadow.hit.height);
        let shared_ok =
            shared_hash(&self.shared_flow[query.shared_flow_start..shadow.shared_start])
                == shadow.shared_hash
                && shared_replay_safe(&self.shared_flow[shadow.shared_start..])
                && (!shadow.ends_iteration || shadow.shared_node == shared);
        if same_key && same_type && registers_ok && shared_ok {
            return;
        }
        self.flow_memo.stats.disagreements += 1;
        if self.flow_memo.stats.disagreements <= 16 {
            eprintln!(
                "[flow-memo-shadow disagreement] file={:?} reference={:?} pos={} flow={:?} origin={:?} walked={result:?} memo={:?} key={same_key} registers={registers_ok} shared={shared_ok} taint={taint:?} before={:?} height={height}/{} flags={}",
                self.binder.source(query.file).file_name, query.reference, self.pos_of(query.reference),
                shadow.flow, shadow.origin.reference, shadow.hit.ty, shadow.before,
                shadow.hit.height, shadow.hit.flags,
            );
        }
    }

    #[inline]
    fn flow_frame_counters(&self) -> FlowCounters {
        FlowCounters {
            instantiation_count: self.instantiation_count,
            total_instantiation_count: self.total_instantiation_count,
            created: self.tables.len()
                + self.binder.transient_symbol_count()
                + self.signatures.len(),
            in_instantiation: !self.active_type_mappers.is_empty(),
        }
    }

    #[inline]
    pub(crate) fn flow_frame_begin(&mut self) {
        self.flow_memo.begin(self.flow_frame_counters());
    }

    #[inline]
    pub(crate) fn flow_frame_end(&mut self) -> (FrameTaint, u16) {
        self.flow_memo.end(self.flow_frame_counters())
    }

    /// A shared-flow answer stands for its original sub-walk's height.
    #[inline]
    pub(crate) fn flow_frame_end_with(&mut self, height: u16) -> FrameTaint {
        self.flow_memo.regs.height = height;
        self.flow_frame_end().0
    }

    #[inline]
    pub(crate) fn flow_memo_consume(&mut self, transient: u32, reference: u32) {
        if transient != UNTAINTED {
            self.flow_memo_consume_transient(transient);
        }
        self.flow_memo.taint_reference(reference);
    }

    #[cold]
    fn flow_memo_consume_transient(&mut self, source: u32) {
        let live = source != 0
            && (self
                .flow_loop_stack
                .iter()
                .any(|entry| entry.serial == source)
                || self.resolution_serials[..self.resolution_targets.len()].contains(&source));
        self.flow_memo.taint(if live { source } else { 0 });
    }

    pub(crate) fn take_flow_type_cache(&mut self) -> SavedFlowTypeCache {
        let saved = SavedFlowTypeCache {
            cache: self.flow_type_cache.take(),
            epoch: self.flow_memo.type_cache_epoch,
        };
        self.flow_memo.type_cache_epoch = self.flow_memo.next_serial();
        saved
    }

    pub(crate) fn restore_flow_type_cache(&mut self, saved: SavedFlowTypeCache) {
        self.flow_type_cache = saved.cache;
        self.flow_memo.type_cache_epoch = saved.epoch;
    }
}

#[cfg(test)]
#[path = "../tests/unit/flow_memo/tests.rs"]
mod tests;
