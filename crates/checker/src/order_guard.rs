//! Order-sensitivity guard for sharded checking (slice W2c/W2e).
//!
//! tsrs-native: tsc has one checker whose type ids are allocated in one global
//! creation order; unions and intersections keep their members sorted by that
//! id, and several operations consume that ORDER (not only the member set):
//! type display, elaboration of the first failing member, representative
//! choice among mutually related candidates, subtype-reduction ties, union and
//! intersection signature lists (overload choice, `infer` over the last
//! signatures), circularity resolution, contextual signature heads. A sharded
//! checker allocates ids in its own local order, so the relative order of two
//! types it created may differ from the serial order, and every such consumer
//! may then differ from tsc.
//!
//! Types created during checker initialization (intrinsics, init globals,
//! augmentation merges) have identical ids in every shard because the
//! initialization sequence is deterministic over the shared snapshot; only
//! types created afterwards ("post-init") are shard-local. The guard is armed
//! by the sharded driver after initialization.
//!
//! Two classes of reasons:
//! - semantic reasons are recorded run-wide and monotonically (speculation
//!   checkpoints never restore them); any of them replays the whole check
//!   serially;
//! - display-class reasons (member order printed, first failing member or
//!   best-matching constituent elaborated) only change diagnostic TEXT, so
//!   they are attached to what they produce — the rendered member-list text
//!   or the relation elaboration chain — and count only if a published
//!   diagnostic contains that text or chain. A relation walk keeps its
//!   elaboration's reasons in its error state, which a later successful
//!   branch resets, so a discarded elaboration marks nothing.
//!
//! The audited site list lives in the W packet (`w-order-guard-audit.md`);
//! completeness of that list is the guard's correctness argument and is
//! reviewed, not assumed.

use rustc_hash::FxHashSet as HashSet;
use std::hash::{Hash, Hasher};

use tsc_diagnostics::{Diagnostic, JsString, MessageChain};
use tsc_types::TypeId;

/// One order-consuming operation class (bit in the reasons set).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OrderReason(u32);

impl OrderReason {
    /// A union or intersection with ≥ 2 post-init members was rendered
    /// (member order is printed). Display-class.
    pub const DISPLAY: Self = Self(1 << 0);
    /// `getCommonSupertype`/`getCommonSubtype` reduced ≥ 2 post-init
    /// candidates (representative choice by reduce-left).
    pub const REPRESENTATIVE: Self = Self(1 << 1);
    /// Subtype reduction removed a member in favour of another shard-local
    /// member (which of the two survives can be an id-order tie).
    pub const SUBTYPE_TIE: Self = Self(1 << 2);
    /// Union call/construct signatures were synthesized from ≥ 2 post-init
    /// members (signature list order feeds overload choice).
    pub const UNION_SIGNATURES: Self = Self(1 << 3);
    /// Intersection members contributed ≥ 2 post-init signature lists
    /// (concatenation order feeds overload choice and `infer`'s last
    /// signatures).
    pub const INTERSECTION_SIGNATURES: Self = Self(1 << 4);
    /// A resolution cycle was detected (which member of the cycle is
    /// entered first depends on the checking order).
    pub const CIRCULARITY: Self = Self(1 << 5);
    /// Best-matching constituent selection over a union target with ≥ 2
    /// post-init members (elaboration choice). Display-class.
    pub const BEST_MATCH: Self = Self(1 << 6);
    /// The shards' initialization did not produce the same type count, so
    /// the "identical pre-guard ids" exemption does not hold (set by the
    /// driver, never by a site).
    pub const INIT_DIVERGENCE: Self = Self(1 << 7);
    /// Relation error elaboration chose the first (or last) failing member
    /// of a union/intersection with ≥ 2 post-init members (F40).
    /// Display-class.
    pub const FIRST_FAILURE: Self = Self(1 << 8);
    /// A contextual signature was built from a union's constituent signatures
    /// with the FIRST constituent as head (`this`/return come from it; F44).
    pub const CONTEXTUAL_SIGNATURE: Self = Self(1 << 9);
    /// Union index infos: two or more resulting infos follow the FIRST
    /// member's declaration order.
    pub const UNION_INDEX_INFOS: Self = Self(1 << 10);
    /// A spread over an intersection folds its LAST constituent.
    pub const INTERSECTION_LAST: Self = Self(1 << 11);

    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Every reason with its feature-only frequency counter (one count per
    /// flagged run and reason; a bitset must never be summed).
    pub const COUNTERS: [(Self, tsc_types::perf::PerfCounter); 12] = [
        (
            Self::DISPLAY,
            tsc_types::perf::PerfCounter::CheckerReplayDisplay,
        ),
        (
            Self::REPRESENTATIVE,
            tsc_types::perf::PerfCounter::CheckerReplayRepresentative,
        ),
        (
            Self::SUBTYPE_TIE,
            tsc_types::perf::PerfCounter::CheckerReplaySubtypeTie,
        ),
        (
            Self::UNION_SIGNATURES,
            tsc_types::perf::PerfCounter::CheckerReplayUnionSignatures,
        ),
        (
            Self::INTERSECTION_SIGNATURES,
            tsc_types::perf::PerfCounter::CheckerReplayIntersectionSignatures,
        ),
        (
            Self::CIRCULARITY,
            tsc_types::perf::PerfCounter::CheckerReplayCircularity,
        ),
        (
            Self::BEST_MATCH,
            tsc_types::perf::PerfCounter::CheckerReplayBestMatch,
        ),
        (
            Self::INIT_DIVERGENCE,
            tsc_types::perf::PerfCounter::CheckerReplayInitDivergence,
        ),
        (
            Self::FIRST_FAILURE,
            tsc_types::perf::PerfCounter::CheckerReplayFirstFailure,
        ),
        (
            Self::CONTEXTUAL_SIGNATURE,
            tsc_types::perf::PerfCounter::CheckerReplayContextualSignature,
        ),
        (
            Self::UNION_INDEX_INFOS,
            tsc_types::perf::PerfCounter::CheckerReplayUnionIndexInfos,
        ),
        (
            Self::INTERSECTION_LAST,
            tsc_types::perf::PerfCounter::CheckerReplayIntersectionLast,
        ),
    ];
}

/// Feature-only trace: with `TSRS_ORDER_GUARD_TRACE=1`, print the reason and a
/// backtrace to stderr the first time a shard sets each reason bit. Never
/// compiled into release timing candidates; the release path is one branch.
#[cfg(feature = "perf-counters")]
fn trace_first_fire(reason: OrderReason) {
    if std::env::var_os("TSRS_ORDER_GUARD_TRACE").is_some() {
        eprintln!(
            "TSRS_ORDER_GUARD first fire: reason bits {:#x} on {:?}\n{}",
            reason.0,
            std::thread::current().id(),
            std::backtrace::Backtrace::force_capture()
        );
    }
}

#[cfg(not(feature = "perf-counters"))]
#[inline(always)]
fn trace_first_fire(_reason: OrderReason) {}

/// Count one flagged run per set reason (feature builds only).
pub(crate) fn count_replay_reasons(reasons: u32) {
    for (reason, counter) in OrderReason::COUNTERS {
        if reasons & reason.bits() != 0 {
            tsc_types::perf::bump(counter);
        }
    }
}

/// Structural hash of a message chain: code and text of every node, in
/// order. A chain nested later under a containing head keeps its own hash,
/// so a marked elaboration is found wherever it is published.
fn chain_key(chain: &MessageChain) -> u64 {
    fn feed(chain: &MessageChain, hasher: &mut std::collections::hash_map::DefaultHasher) {
        chain.code.hash(hasher);
        chain.text.hash(hasher);
        chain.next.len().hash(hasher);
        for next in &chain.next {
            feed(next, hasher);
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    feed(chain, &mut hasher);
    hasher.finish()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Display-class observations attached to what they produced.
#[derive(Clone, Debug, Default)]
pub(crate) struct DisplayMarks {
    /// Rendered union/intersection member lists (verbatim text).
    texts: Vec<JsString>,
    /// Chains of relation elaborations that chose a member by order.
    chains: HashSet<u64>,
    /// Union of the display-class reasons behind the marks.
    reasons: u32,
}

impl DisplayMarks {
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty() && self.chains.is_empty()
    }

    /// Number of recorded observations (texts and chains); grows
    /// monotonically, so a later value larger than an earlier one means the
    /// checker rendered order-sensitive display text in between.
    pub fn len(&self) -> usize {
        self.texts.len() + self.chains.len()
    }

    pub const fn reasons(&self) -> u32 {
        self.reasons
    }

    pub fn extend(&mut self, other: &DisplayMarks) {
        self.texts.extend(other.texts.iter().cloned());
        self.chains.extend(other.chains.iter().copied());
        self.reasons |= other.reasons;
    }

    /// Whether a published diagnostic contains a marked elaboration chain or
    /// a marked rendered text, in its head, its nested chain or its related
    /// information.
    pub fn is_marked(&self, diagnostic: &Diagnostic) -> bool {
        if self.is_empty() {
            return false;
        }
        let mut chains = vec![&diagnostic.message];
        chains.extend(diagnostic.related.iter().map(|related| &related.message));
        while let Some(chain) = chains.pop() {
            if self.chains.contains(&chain_key(chain)) {
                return true;
            }
            if self
                .texts
                .iter()
                .any(|text| contains_bytes(chain.text.as_bytes(), text.as_bytes()))
            {
                return true;
            }
            chains.extend(chain.next.iter());
        }
        false
    }
}

/// Per-checker-state guard. Disarmed states (the serial checker) record
/// nothing. The semantic reasons set is monotonic for the whole run:
/// speculation checkpoints do not capture or restore it, and no site clears
/// it.
#[derive(Clone, Debug, Default)]
pub(crate) struct OrderGuard {
    armed: bool,
    /// First shard-local type id: every id below it was created by the
    /// deterministic initialization sequence.
    init_boundary: u32,
    reasons: u32,
    /// Structural display-class choices made outside a relation walk's
    /// error state (best match from an elaboration driver); the next
    /// relation walk that publishes an elaboration adopts them.
    pending_structural: u32,
    marks: DisplayMarks,
}

impl OrderGuard {
    /// Arm the guard: `boundary` is the type count right after checker
    /// initialization.
    pub fn arm(&mut self, boundary: usize) {
        self.armed = true;
        self.init_boundary = u32::try_from(boundary).expect("type count fits u32");
    }

    /// Semantic reasons recorded so far (0 = none).
    pub const fn reasons(&self) -> u32 {
        self.reasons
    }

    /// Whether `ty` is shard-local (created after the guard was armed).
    /// Always false for a disarmed guard, so callers can count without
    /// branching on the armed state themselves.
    #[inline]
    pub fn is_post_init(&self, ty: TypeId) -> bool {
        self.armed && ty.index() >= self.init_boundary
    }

    /// Whether at least two of `types` are shard-local (false when disarmed).
    #[inline]
    pub fn two_post_init(&self, types: impl IntoIterator<Item = TypeId>) -> bool {
        if !self.armed {
            return false;
        }
        let mut post_init = 0u32;
        for ty in types {
            if self.is_post_init(ty) {
                post_init += 1;
                if post_init >= 2 {
                    return true;
                }
            }
        }
        false
    }

    /// Record the semantic `reason` when at least two of `types` are
    /// shard-local.
    #[inline]
    pub fn note(&mut self, reason: OrderReason, types: impl IntoIterator<Item = TypeId>) {
        if self.two_post_init(types) {
            self.set(reason);
        }
    }

    /// Record the semantic `reason` unconditionally (operations whose order
    /// dependence is not expressed through type ids, e.g. circularity).
    #[inline]
    pub fn note_always(&mut self, reason: OrderReason) {
        if self.armed {
            self.set(reason);
        }
    }

    #[inline]
    fn set(&mut self, reason: OrderReason) {
        if self.reasons & reason.0 == 0 {
            self.reasons |= reason.0;
            trace_first_fire(reason);
        }
    }

    /// Display-class: the rendered member-list `text` of a union or
    /// intersection with ≥ 2 shard-local members. A published diagnostic
    /// containing this text (head, nested or related) triggers the replay.
    #[inline]
    pub fn mark_display_text(
        &mut self,
        text: &JsString,
        members: impl IntoIterator<Item = TypeId>,
    ) {
        if self.two_post_init(members) {
            self.marks.texts.push(text.clone());
            self.marks.reasons |= OrderReason::DISPLAY.0;
        }
    }

    /// Display-class structural choice made outside a relation walk; the
    /// next published elaboration adopts it.
    #[inline]
    pub fn note_structural(
        &mut self,
        reason: OrderReason,
        types: impl IntoIterator<Item = TypeId>,
    ) {
        if self.two_post_init(types) {
            self.pending_structural |= reason.0;
        }
    }

    /// Take the pending structural display-class reasons (0 when none).
    #[inline]
    pub fn take_structural(&mut self) -> u32 {
        std::mem::take(&mut self.pending_structural)
    }

    /// Display-class: a relation elaboration `chain` that chose members by
    /// order. A published diagnostic containing this chain (as head, nested
    /// or related information) triggers the replay.
    pub fn mark_chain(&mut self, chain: &MessageChain, reasons: u32) {
        if self.armed && reasons != 0 {
            self.marks.chains.insert(chain_key(chain));
            self.marks.reasons |= reasons;
        }
    }

    pub fn marks(&self) -> &DisplayMarks {
        &self.marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_diagnostics::{DiagnosticCategory, RelatedInfo};

    fn chain(code: u32, text: &str, next: Vec<MessageChain>) -> MessageChain {
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: text.to_owned().into(),
            key: None,
            args: Vec::new(),
            next_present: !next.is_empty(),
            next,
            related: Vec::new(),
            repopulate: None,
        }
    }

    #[test]
    fn disarmed_guard_records_nothing() {
        let mut guard = OrderGuard::default();
        guard.note(
            OrderReason::REPRESENTATIVE,
            [TypeId::new(5), TypeId::new(6)],
        );
        guard.note_always(OrderReason::CIRCULARITY);
        guard.mark_display_text(&"A | B".to_owned().into(), [TypeId::new(5), TypeId::new(6)]);
        guard.mark_chain(
            &chain(2322, "x", Vec::new()),
            OrderReason::FIRST_FAILURE.bits(),
        );
        assert_eq!(guard.reasons(), 0);
        assert!(guard.marks().is_empty());
        assert!(
            !guard.is_post_init(TypeId::new(0)),
            "disarmed: nothing is shard-local"
        );
    }

    #[test]
    fn armed_guard_needs_two_post_init_types() {
        let mut guard = OrderGuard::default();
        guard.arm(10);
        guard.note(
            OrderReason::REPRESENTATIVE,
            [TypeId::new(1), TypeId::new(2)],
        );
        assert_eq!(guard.reasons(), 0, "two init types");
        guard.note(
            OrderReason::REPRESENTATIVE,
            [TypeId::new(1), TypeId::new(10)],
        );
        assert_eq!(guard.reasons(), 0, "one post-init type");
        guard.note(
            OrderReason::REPRESENTATIVE,
            [TypeId::new(10), TypeId::new(3), TypeId::new(11)],
        );
        assert_eq!(guard.reasons(), OrderReason::REPRESENTATIVE.bits());
        guard.note_always(OrderReason::CIRCULARITY);
        assert_eq!(
            guard.reasons(),
            OrderReason::REPRESENTATIVE.bits() | OrderReason::CIRCULARITY.bits()
        );
    }

    #[test]
    fn display_marks_match_published_text_and_nested_or_related_chains() {
        let mut guard = OrderGuard::default();
        guard.arm(10);
        guard.mark_display_text(
            &"Beta | Alpha".to_owned().into(),
            [TypeId::new(10), TypeId::new(11)],
        );
        guard.mark_display_text(
            &"C | D".to_owned().into(),
            [TypeId::new(1), TypeId::new(11)],
        );
        let elaboration = chain(
            2322,
            "Type 'Beta' is not assignable to type 'number'.",
            Vec::new(),
        );
        guard.mark_chain(&elaboration, OrderReason::FIRST_FAILURE.bits());
        let marks = guard.marks().clone();
        assert_eq!(
            marks.reasons(),
            OrderReason::DISPLAY.bits() | OrderReason::FIRST_FAILURE.bits()
        );
        // Head text containing the rendered union.
        let head = Diagnostic::new(
            None,
            None,
            None,
            chain(
                2322,
                "Type 'Beta | Alpha' is not assignable to type 'number'.",
                Vec::new(),
            ),
        );
        assert!(marks.is_marked(&head));
        // The one-post-init text was never marked.
        let other = Diagnostic::new(
            None,
            None,
            None,
            chain(2322, "Type 'C | D' is bad.", Vec::new()),
        );
        assert!(!marks.is_marked(&other));
        // Elaboration nested under a containing head (rewritten chain).
        let nested = Diagnostic::new(
            None,
            None,
            None,
            chain(2345, "Argument mismatch.", vec![elaboration.clone()]),
        );
        assert!(marks.is_marked(&nested));
        // Elaboration carried as related information.
        let mut related =
            Diagnostic::new(None, None, None, chain(2769, "No overload.", Vec::new()));
        related.related.push(RelatedInfo {
            file_name: None,
            start: None,
            length: None,
            message: elaboration,
        });
        assert!(marks.is_marked(&related));
        let clean = Diagnostic::new(
            None,
            None,
            None,
            chain(2304, "Cannot find name 'x'.", Vec::new()),
        );
        assert!(!marks.is_marked(&clean));
    }
}
