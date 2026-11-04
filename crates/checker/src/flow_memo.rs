//! Flow-walk dependency registers, following tsrs's exact flow memo.
//!
//! There is deliberately no memo table, key, lookup or store yet. Each flow
//! level saves the registers in a reusable stack; a child reports its oldest
//! dependencies, flags and height to its parent. Nested queries start their
//! own height at zero but still report dependencies to the enclosing walk.

use rustc_hash::FxHashMap;
use tsc_syntax::NodeId;
use tsc_types::TypeId;

use crate::state::CheckerState;

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

pub(crate) struct FlowMemo {
    serial: u32,
    regs: Regs,
    frames: Vec<FlowFrame>,
    /// Saved/restored with flow_type_cache; an entry replacement gets a
    /// fresh epoch too. UNTAINTED identifies the initial, empty cache.
    pub(crate) type_cache_epoch: u32,
}

impl FlowMemo {
    pub(crate) fn new() -> Self {
        Self {
            serial: 1,
            regs: FRESH_REGS,
            frames: Vec::new(),
            type_cache_epoch: UNTAINTED,
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
        if counters.total_instantiation_count != before.total_instantiation_count
            || before.in_instantiation
            || counters.created != before.created
        {
            self.add_flags(FLAG_EFFECTS);
        }
        if before.instantiation_count == 0
            || counters.instantiation_count != before.instantiation_count
        {
            self.add_flags(FLAG_COUNT_RESET);
        }
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
}

pub(crate) struct SavedFlowTypeCache {
    cache: Option<FxHashMap<NodeId, TypeId>>,
    epoch: u32,
}

impl CheckerState<'_> {
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
