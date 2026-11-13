//! Bounded, per-checker flow memo and dependency registers (tsrs C-0).
//! Complete, independent sub-walks can reuse their result. `flow-memo-shadow`
//! validates candidate hits with the real walk instead of replacing it.

use std::hash::{Hash, Hasher};
#[cfg(any(test, feature = "flow-memo-shadow"))]
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};
use tsc_binder::flow::FlowId;
#[cfg(test)]
use tsc_syntax::NodeData;
use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::{FlowFlags, NodeFlags, TypeId};

use crate::flow::{FlowQuery, FlowType, SharedFlow};
use crate::state::CheckerState;

pub(crate) const FLOW_DEPTH_LIMIT: u32 = 2000;
const TABLE_SLOTS: usize = 1 << 12;
pub(crate) const UNTAINTED: u32 = u32::MAX;
/// Instantiated, began under an active mapper, or created an identity.
pub(crate) const FLAG_EFFECTS: u8 = 1;
/// Consulted the expression flow cache, including a miss or absent cache.
/// The answer depends on its epoch.
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

#[must_use]
pub(crate) struct FlowFrame {
    start: u32,
    saved: Regs,
    counters: FlowCounters,
    #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
    depth: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTaint {
    pub(crate) transient: u32,
    pub(crate) reference: u32,
    pub(crate) flags: u8,
}

/// A reference query's immutable identity, including its owning flow file.
#[derive(Clone)]
pub(crate) struct MemoKey {
    #[cfg(any(test, target_pointer_width = "32"))]
    hash: u128,
    #[cfg(target_pointer_width = "64")]
    node_base: [u64; 2],
    #[cfg(target_pointer_width = "32")]
    file: usize,
    #[cfg(any(test, feature = "flow-memo-shadow"))]
    full: Arc<[u8]>,
}

impl MemoKey {
    fn new(
        file: usize,
        hash: u128,
        #[cfg(any(test, feature = "flow-memo-shadow"))] full: Arc<[u8]>,
    ) -> Self {
        #[cfg(target_pointer_width = "64")]
        let node_base = {
            let zero = hash_key(&(file, FlowId::new(0), hash));
            [
                (zero as u64).rotate_right(26),
                ((zero >> 64) as u64).rotate_right(26),
            ]
        };
        Self {
            #[cfg(any(test, target_pointer_width = "32"))]
            hash,
            #[cfg(target_pointer_width = "64")]
            node_base,
            #[cfg(target_pointer_width = "32")]
            file,
            #[cfg(any(test, feature = "flow-memo-shadow"))]
            full,
        }
    }

    #[inline]
    fn at_node(&self, flow: FlowId) -> u128 {
        #[cfg(target_pointer_width = "64")]
        {
            // The locked Fx integer hash adds a word and multiplies by K.
            // In (file, flow, key.low, key.high), flow has three multiplies
            // left. Preparing the zero-flow sums once preserves every tag
            // and table collision while removing the invariant arithmetic.
            const K: u64 = 0xf135_7aea_2e62_a9c5;
            const FLOW_FACTOR: u64 = K.wrapping_mul(K).wrapping_mul(K);
            let delta = u64::from(flow.index()).wrapping_mul(FLOW_FACTOR);
            let low = self.node_base[0].wrapping_add(delta).rotate_left(26);
            let high = self.node_base[1].wrapping_add(delta).rotate_left(26);
            u128::from(low) | (u128::from(high) << 64)
        }
        #[cfg(target_pointer_width = "32")]
        {
            hash_key(&(self.file, flow, self.hash))
        }
    }
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

#[cfg(any(test, feature = "flow-memo-shadow"))]
#[derive(Clone)]
struct ShadowOrigin {
    key: Arc<[u8]>,
    file: usize,
    flow: FlowId,
    reference: NodeId,
}

#[cfg(any(test, feature = "flow-memo-shadow"))]
struct ShadowHit {
    flow: FlowId,
    hit: MemoHit,
    origin: ShadowOrigin,
    before: FrameTaint,
    height_before: u16,
    created_before: (usize, usize, usize),
    shared_start: usize,
    shared_hash: u64,
    ends_iteration: bool,
    shared_node: Option<FlowId>,
}

pub(crate) struct FrameMemo {
    key: MemoKey,
    checkpoints: usize,
    #[cfg(any(test, feature = "flow-memo-shadow"))]
    shadow: usize,
    shared_start: usize,
}

#[cfg(any(test, feature = "flow-memo-shadow"))]
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
    #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
    frame_depth: usize,
    /// Saved/restored with flow_type_cache; an entry replacement gets a
    /// fresh epoch too. UNTAINTED identifies the initial, empty cache.
    pub(crate) type_cache_epoch: u32,
    slots: Vec<Slot>,
    #[cfg(feature = "flow-memo-shadow")]
    shadow_slots: Vec<Option<ShadowOrigin>>,
    checkpoints: Vec<FlowId>,
    /// Reused only while constructing a structural key; no resolving calls.
    key_buffer: Vec<u8>,
    #[cfg(any(test, feature = "flow-memo-shadow"))]
    shadow_hits: Vec<ShadowHit>,
    #[cfg(any(test, feature = "flow-memo-shadow"))]
    stats: MemoStats,
    #[cfg(feature = "flow-memo-shadow")]
    reported: std::cell::Cell<(u64, u64, u64, u64)>,
}

impl FlowMemo {
    pub(crate) fn new() -> Self {
        Self {
            serial: 1,
            regs: FRESH_REGS,
            #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
            frame_depth: 0,
            type_cache_epoch: UNTAINTED,
            slots: Vec::new(),
            #[cfg(feature = "flow-memo-shadow")]
            shadow_slots: Vec::new(),
            checkpoints: Vec::new(),
            key_buffer: Vec::new(),
            #[cfg(any(test, feature = "flow-memo-shadow"))]
            shadow_hits: Vec::new(),
            #[cfg(any(test, feature = "flow-memo-shadow"))]
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
    fn begin(&mut self, counters: FlowCounters) -> FlowFrame {
        let start = self.next_serial();
        let frame = FlowFrame {
            start,
            saved: self.regs,
            counters,
            #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
            depth: self.frame_depth,
        };
        #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
        {
            self.frame_depth += 1;
        }
        self.regs = FRESH_REGS;
        if self.serial == UNTAINTED - 1 {
            self.taint(0);
        }
        frame
    }

    /// Transparent frames (C-1) propagate children without adding a flow
    /// level. Dependencies born inside a frame do not taint its answer,
    /// but their serials still propagate to older enclosing frames.
    #[inline]
    pub(crate) fn end_transparent(&mut self, frame: FlowFrame) -> (FrameTaint, u16, u8) {
        #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
        {
            assert_eq!(self.frame_depth, frame.depth + 1, "flow frame order");
            self.frame_depth = frame.depth;
        }
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
    fn end(&mut self, frame: FlowFrame, counters: FlowCounters) -> (FrameTaint, u16) {
        self.add_flags(counter_flags(frame.counters, counters));
        let (taint, height, _) = self.end_transparent(frame);
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
        #[cfg(any(test, debug_assertions, feature = "flow-memo-shadow"))]
        {
            self.frame_depth
        }
        #[cfg(not(any(test, debug_assertions, feature = "flow-memo-shadow")))]
        {
            0
        }
    }

    /// Answer slots and walk scratch owned by the normal checker. The
    /// observation feature reports its additional key/origin storage.
    pub(crate) fn memory_usage(&self) -> (usize, usize) {
        (
            self.slots.iter().filter(|slot| slot.ty.is_some()).count(),
            self.slots.capacity() * std::mem::size_of::<Slot>()
                + self.checkpoints.capacity() * std::mem::size_of::<FlowId>()
                + self.key_buffer.capacity(),
        )
    }

    #[cfg(feature = "flow-memo-shadow")]
    fn frame_taint(&self, frame: &FlowFrame, counters: FlowCounters) -> FrameTaint {
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

    fn node_key(flow: FlowId, key: &MemoKey) -> u128 {
        key.at_node(flow)
    }

    fn slot_index(key: u128) -> usize {
        ((key as u64) ^ ((key >> 64) as u64).rotate_left(29)) as usize & (TABLE_SLOTS - 1)
    }

    fn lookup(&mut self, flow: FlowId, key: &MemoKey) -> Option<MemoHit> {
        #[cfg(any(test, feature = "flow-memo-shadow"))]
        {
            self.stats.lookups += 1;
        }
        let key = Self::node_key(flow, key);
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
        #[cfg(not(feature = "flow-memo-shadow"))]
        let _ = query;
        if self.slots.is_empty() {
            self.slots.resize(TABLE_SLOTS, Slot::default());
            #[cfg(feature = "flow-memo-shadow")]
            {
                self.shadow_slots.resize(TABLE_SLOTS, None);
            }
        }
        let node_key = Self::node_key(flow, key);
        let index = Self::slot_index(node_key);
        #[cfg(any(test, feature = "flow-memo-shadow"))]
        {
            self.stats.occupied += usize::from(self.slots[index].ty.is_none());
        }
        self.slots[index] = Slot {
            key: node_key,
            ty: Some(hit.ty),
            epoch: hit.epoch,
            height: hit.height,
            flags: hit.flags,
        };
        #[cfg(any(test, feature = "flow-memo-shadow"))]
        {
            self.stats.stores += 1;
        }
        #[cfg(feature = "flow-memo-shadow")]
        {
            self.shadow_slots[index] = Some(ShadowOrigin {
                key: Arc::clone(&key.full),
                file: query.file_index(),
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

#[cfg(any(test, feature = "flow-memo-shadow"))]
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
    /// Existing shared-flow entries are inputs to later sub-walks. Even
    /// when consuming one creates no identities, its stored effects and
    /// reference dependencies must reach the enclosing flow frame. A memo
    /// replacement cannot skip that propagation.
    pub(crate) fn record_shared_flow(&mut self, query: &mut FlowQuery, shared: SharedFlow) {
        if !shared_replay_safe(std::slice::from_ref(&shared)) {
            query.impure_shared += 1;
        }
        self.shared_flow.push(shared);
    }

    /// Only reads cached links and syntax. In particular, never resolves a
    /// symbol or assigns a node/symbol identity while constructing a key.
    pub(crate) fn flow_memo_key(&mut self, query: &FlowQuery) -> Option<MemoKey> {
        if query.synthetic_props.is_some() || query.synthetic_this_root {
            return None;
        }
        // Four 32-bit identities fit exactly. Position zero cannot name a
        // symbol and denotes `this`; zero is also the absent container.
        // Hashed property chains reserve bit 95, outside packed keys when
        // the declared type index is below 2^31.
        let root = match self.kind_of(query.reference) {
            SyntaxKind::Identifier => {
                let symbol = self
                    .links
                    .node(query.reference)
                    .resolved_symbol
                    .resolved()?;
                if symbol == self.unknown_symbol || self.is_this_in_type_query(query.reference) {
                    return None;
                }
                Some(symbol.position())
            }
            SyntaxKind::ThisKeyword => Some(0),
            _ => None,
        };
        let packed = root
            .filter(|_| query.declared_type.index() < (1 << 31))
            .map(|root| {
                (u128::from(root) << 96)
                    | (u128::from(query.declared_type.index()) << 64)
                    | (u128::from(query.initial_type.index()) << 32)
                    | u128::from(query.flow_container.map_or(0, NodeId::position))
            });
        #[cfg(not(any(test, feature = "flow-memo-shadow")))]
        if let Some(hash) = packed {
            return Some(MemoKey::new(query.file_index(), hash));
        }
        // An identifier root is one tag plus a u32, followed by the three
        // fixed u32 key fields. Property fragments reserve their bytes too.
        let mut bytes = std::mem::take(&mut self.flow_memo.key_buffer);
        bytes.clear();
        bytes.reserve(1 + 4 + 3 * 4);
        // Restore the buffer on rejection too: a partial reference key is
        // discarded by the next clear, without losing its allocation.
        let result = self
            .write_flow_memo_reference(query.reference, &mut bytes)
            .map(|()| {
                bytes.extend_from_slice(&query.declared_type.index().to_le_bytes());
                bytes.extend_from_slice(&query.initial_type.index().to_le_bytes());
                bytes.extend_from_slice(
                    &query
                        .flow_container
                        .map_or(0, NodeId::position)
                        .to_le_bytes(),
                );
                // Like tsrs, inline/reduce modes gate frames rather than being
                // key fields. Synthetic modes have no structural reference key.
                MemoKey::new(
                    query.file_index(),
                    packed.unwrap_or_else(|| hash_key(&bytes) | (1 << 95)),
                    #[cfg(any(test, feature = "flow-memo-shadow"))]
                    bytes.as_slice().into(),
                )
            });
        self.flow_memo.key_buffer = bytes;
        result
    }

    fn write_flow_memo_reference(&self, node: NodeId, bytes: &mut Vec<u8>) -> Option<()> {
        match_record! { self.record_of(node), {
            as_identifier(_) => {
                let symbol = self.links.node(node).resolved_symbol.resolved()?;
                if symbol == self.unknown_symbol || self.is_this_in_type_query(node) {
                    return None;
                }
                bytes.push(b'I');
                bytes.extend_from_slice(&symbol.position().to_le_bytes());
            },
            as_property_access_expression(data) => {
                if self
                    .binder
                    .flags_of(node)
                    .intersects(NodeFlags::OPTIONAL_CHAIN)
                {
                    return None;
                }
                self.write_flow_memo_reference(data.expression?, bytes)?;
                let name = self.escaped_text_of(data.name)?;
                // Include the fixed suffix in the reservation so appending
                // it after the final property cannot immediately regrow.
                bytes.reserve(1 + 4 + name.len() + 3 * 4);
                bytes.push(b'P');
                bytes.extend_from_slice(&u32::try_from(name.len()).ok()?.to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
            },
            _ if self.kind_of(node) == SyntaxKind::ThisKeyword => bytes.push(b'T'),
            _ => return None,
        }}
        Some(())
    }

    pub(crate) fn flow_frame_memo(&self, query: &FlowQuery) -> Option<FrameMemo> {
        if self.inline_level != 0
            || query.reduce_depth != 0
            || !self.reduce_label_overrides.is_empty()
            || self.flow_analysis_disabled
        {
            return None;
        }
        Some(FrameMemo {
            key: query.memo_key.clone()?,
            checkpoints: self.flow_memo.checkpoints.len(),
            #[cfg(any(test, feature = "flow-memo-shadow"))]
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

    /// Reuse a complete answer whose dependencies remain valid. In shadow
    /// builds each candidate is kept until its real frame finishes.
    #[cfg_attr(feature = "flow-memo-shadow", allow(clippy::too_many_arguments))]
    pub(crate) fn flow_memo_at_node(
        &mut self,
        query: &FlowQuery,
        memo: &FrameMemo,
        entry: FlowId,
        flow: FlowId,
        flags: FlowFlags,
        shared: Option<FlowId>,
        #[cfg(feature = "flow-memo-shadow")] frame: &FlowFrame,
    ) -> Option<(FlowType, bool)> {
        #[cfg(not(feature = "flow-memo-shadow"))]
        let _ = shared;
        let ends_iteration = flags.intersects(
            FlowFlags::TRUE_CONDITION | FlowFlags::FALSE_CONDITION | FlowFlags::SWITCH_CLAUSE,
        ) || flags.intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL)
            && self.flow_label_antecedents(query.file_index(), flow).len() > 1;
        if flow != entry && !ends_iteration {
            if !is_checkpoint(query.file_index(), flow) {
                return None;
            }
            self.flow_memo.checkpoints.push(flow);
        }
        let hit = self.flow_memo.lookup(flow, &memo.key)?;
        if !self.flow_memo_consult_ok(query, hit) {
            return None;
        }
        #[cfg(any(test, feature = "flow-memo-shadow"))]
        {
            self.flow_memo.stats.hits += 1;
        }
        #[cfg(feature = "flow-memo-shadow")]
        {
            let index = FlowMemo::slot_index(FlowMemo::node_key(flow, &memo.key));
            let origin = self.flow_memo.shadow_slots[index]
                .as_ref()
                .expect("shadow slot")
                .clone();
            self.flow_memo.shadow_hits.push(ShadowHit {
                flow,
                hit,
                origin,
                before: self
                    .flow_memo
                    .frame_taint(frame, self.flow_frame_counters()),
                height_before: self.flow_memo.regs.height,
                created_before: (
                    self.tables.len(),
                    self.binder.transient_symbol_count(),
                    self.signatures.len(),
                ),
                shared_start: self.shared_flow.len(),
                shared_hash: shared_hash(&self.shared_flow[query.shared_flow_start..]),
                ends_iteration,
                shared_node: memo_shared_node(ends_iteration, shared),
            });
            None
        }
        #[cfg(not(feature = "flow-memo-shadow"))]
        {
            self.flow_memo.raise_height(hit.height);
            self.flow_memo.add_flags(hit.flags);
            Some((FlowType::Type(hit.ty), ends_iteration))
        }
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
        #[cfg(not(any(test, feature = "flow-memo-shadow")))]
        let _ = shared;
        // Settle BEFORE filling: nested frames can replace any direct-mapped
        // slot, so each pending hit owns the answer and full key it found.
        #[cfg(any(test, feature = "flow-memo-shadow"))]
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
                // Cache misses affect the executed path too. Bind those
                // answers to the epoch, while a walk that never consulted
                // the expression cache can survive its scoped reset.
                epoch: if taint.flags & FLAG_TYPE_CACHE == 0 {
                    0
                } else {
                    self.flow_memo.type_cache_epoch
                },
                // Keep conservative dependency propagation on replacement:
                // later warm paths may consult an expression-cache entry.
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

    #[cfg(any(test, feature = "flow-memo-shadow"))]
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
            && shadow.origin.file == query.file_index()
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
            let (types, symbols, signatures) = shadow.created_before;
            for cached in self.shared_flow[query.shared_flow_start..shadow.shared_start]
                .iter()
                .filter(|s| {
                    s.flags & FLAG_EFFECTS != 0
                        || s.reference != UNTAINTED
                        || s.transient != UNTAINTED
                })
                .take(16)
            {
                eprintln!("[flow-memo-shadow existing-shared] flow={:?} type={:?} flags={} transient={} reference={}", cached.flow, cached.flow_type, cached.flags, cached.transient, cached.reference);
            }

            eprintln!("[flow-memo-shadow creations] types={types}->{} symbols={symbols}->{} signatures={signatures}->{}", self.tables.len(), self.binder.transient_symbol_count(), self.signatures.len());
            for index in types..self.tables.len().min(types + 8) {
                let ty = TypeId::new(index as u32);
                eprintln!(
                    "[flow-memo-shadow new-type] {ty:?} {:?}",
                    self.tables.type_of(ty)
                );
            }
            for index in symbols..self.binder.transient_symbol_count().min(symbols + 8) {
                let symbol =
                    tsc_binder::SymbolId::new(tsc_types::TRANSIENT_SYMBOL_BIT | index as u32);
                eprintln!(
                    "[flow-memo-shadow new-symbol] {symbol:?} {:?}",
                    self.binder.symbol(symbol)
                );
            }
        }

        if self.flow_memo.stats.disagreements <= 16 {
            eprintln!(
                "[flow-memo-shadow disagreement] file={:?} reference={:?} pos={} flow={:?} origin={:?} walked={result:?} memo={:?} key={same_key} registers={registers_ok} shared={shared_ok} taint={taint:?} before={:?} height={height}/{} flags={}",
                self.binder.source(query.file_index()).file_name, query.reference, self.pos_of(query.reference),
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
    pub(crate) fn flow_frame_begin(&mut self) -> FlowFrame {
        self.flow_memo.begin(self.flow_frame_counters())
    }

    #[inline]
    pub(crate) fn flow_frame_end(&mut self, frame: FlowFrame) -> (FrameTaint, u16) {
        self.flow_memo.end(frame, self.flow_frame_counters())
    }

    /// A shared-flow answer stands for its original sub-walk's height.
    #[inline]
    pub(crate) fn flow_frame_end_with(&mut self, frame: FlowFrame, height: u16) -> FrameTaint {
        self.flow_memo.regs.height = height;
        self.flow_frame_end(frame).0
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
