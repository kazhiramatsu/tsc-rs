//! M6 7.0t: the speculation scoped-transaction — the M6 START
//! PRECONDITION (m6-inference-calls-steps.md Stage 7.0t;
//! definition-of-done.md checkpoint table row "M6 start"; spec input =
//! the 2026-07-19 review's STATE-SURFACE INVENTORY, transcribed there).
//!
//! tsrs-native: tsc has NO checker-level speculation transaction. Its
//! candidate trials stay clean through four ported mechanisms instead:
//! checkMode-driven cache bypass (checkExpressionCached 80581 neither
//! reads nor writes links.resolvedType under any non-Normal mode),
//! reportErrors=false error COLLECTION during applicability
//! (chooseOverload 76763 — relation errors become values, never
//! diagnostics), per-candidate fresh InferenceContexts (76809), and
//! clearActiveMapperCaches at inference-fixing time (73624). The port
//! needs the explicit transaction on top because (a) a CheckAbort
//! unwind can abort a trial at ANY depth (tsc has no such exit), and
//! (b) the port's addLazyDiagnostic identity is EAGER, so trial-time
//! sink pushes exist where tsc defers them.
//!
//! The transaction's contract, by inventory category:
//! - A (transient stacks): truncated/restored to the checkpoint marks
//!   on rollback; debug-asserted BALANCED on commit.
//! - B (counters): `speculation_depth` is the transaction's own RAII
//!   guard; `instantiation_depth`/`inline_level`/variance flags/
//!   `suggestion_count`/`is_inference_partially_blocked` restore on
//!   rollback. `instantiation_count` deliberately does NOT restore —
//!   tsc resets it at the three check entry points (86551/86921/80965)
//!   and never mid-resolution. `flow_analysis_disabled` is a one-way
//!   latch even in tsc — left alone.
//! - C (permanent-truth caches, links, interners): rollback-capable
//!   candidate trials never publish cold Links/Signature cache entries.
//!   Pure caches bypass their write; protocols that need a temporary
//!   re-entrancy sentinel or stable identity (call resolvedSignature,
//!   type-node resolution, and structured members) journal and restore
//!   the entry slot at either boundary. A completed candidate, selected
//!   OR rejected, keeps its declaration SignatureIds, completed signature
//!   return types, and contextual-function state (`ContextChecked` plus
//!   contextual parameter symbol types). tsc shares those once-results
//!   across both relation passes; the typed `Reject` outcome preserves
//!   them while still rolling back trial-only state. Nested retention
//!   promotes the original snapshots to the parent transaction. Fresh
//!   semantic object initialization is not a cache publication.
//!   Structurally keyed type interners remain monotone and
//!   candidate-independent, as in tsc.
//! - D (diagnostics sinks): truncated to the checkpoint marks on
//!   rollback (push-dedupe is order-safe under truncation), kept on
//!   commit. Explicit journals preserve completed diagnostics from
//!   reporting-mode iteration walks and contextual-function once-checks:
//!   tsc runs both eagerly while trying an overload and has no transaction
//!   to remove them when the candidate fails. The contextual journal is
//!   retained only for a completed `Reject`, alongside its category-C
//!   once-state; a true rollback or abort drops it. `deferred_nodes`
//!   deliberately survives rollback
//!   — tsc checkNodeDeferred (86899-86908) registers unconditionally,
//!   and deferred nodes registered under a failed candidate are still
//!   checked (verified against 6.0.3 source; the inventory's VERIFY
//!   item).
//!
//! Boundary ordering rule: `speculate` rolls back BEFORE re-propagating
//! an Err, so by the time outer frames' Err-revert twins fire,
//! `speculation_depth` is already back at its entry value. Revert twins
//! therefore never assert the depth (they RESTORE state, which is
//! always legal) — the convention the review's B35 item asked to pick,
//! resolved by dropping the one assert that disagreed
//! (revert_node_enum_values_computed).

use std::collections::{HashMap, HashSet};

use tsc_binder::flow::FlowId;
use tsc_syntax::NodeId;
use tsc_types::TypeId;

use crate::links::SpeculativeLinksMarks;
use crate::state::{CheckResult, CheckerState};

/// What a completed speculative region wants done with the state it
/// accumulated: `Commit` selects the candidate and retains its
/// commit-class state; `Rollback` restores the checkpoint because the
/// candidate failed and chooseOverload continues to the next one.
pub enum SpeculationOutcome<T> {
    Commit(T),
    /// The overload candidate was semantically completed but rejected.
    /// Drop trial-only state and unrelated diagnostics while retaining the
    /// declaration/contextual once-results, including their completed
    /// diagnostics, that tsc leaves shared across later candidates and
    /// relation passes.
    Reject(T),
    Rollback(T),
}

/// One balanced reporting-mode iteration capture. Every entry carries
/// sink marks: a nested entry may complete inside an enclosing
/// iteration walk before a nested overload transaction rolls back, so
/// its rows must reach the journal at that inner completion boundary.
/// Only the outermost entry carries journal marks; when it completes,
/// provisional nested clones are discarded and the surviving sink
/// suffix is captured again with final related information.
#[must_use = "an iteration diagnostic capture must be ended"]
pub(crate) struct TscEagerIterationCapture {
    active: bool,
    depth: usize,
    sink_marks: Option<(usize, usize)>,
    journal_marks: Option<(usize, usize)>,
}

/// Everything `begin_speculation` captures. Vec-backed state stores its
/// length (truncate-to-mark restoration: entries above the mark are the
/// trial's own, entries below it were only mutated in ways that persist
/// in tsc too — e.g. resolution_results cycle flags); map/set-backed
/// state whose entries the trial may remove or overwrite stores a
/// clone (they are empty or near-empty at every real boundary).
#[must_use = "a speculation checkpoint must be committed or rolled back"]
pub struct SpeculationCheckpoint {
    /// speculation_depth AFTER the begin increment; commit/rollback
    /// assert it still holds, enforcing LIFO transaction nesting.
    depth: u32,
    resolved: bool,
    speculative_links: SpeculativeLinksMarks,
    speculative_signature_returns: usize,

    // ---- A: transient stacks ----
    resolution_targets: usize,
    resolution_results: usize,
    resolution_property_names: usize,
    resolution_start: usize,
    contextual_type_nodes: usize,
    contextual_types: usize,
    contextual_is_cache: usize,
    contextual_binding_patterns: usize,
    inference_context_nodes: usize,
    inference_contexts: usize,
    awaited_type_stack: usize,
    active_type_mappers: usize,
    active_type_mappers_caches: usize,
    variance_handler_stack: usize,
    class_interface_declared_in_progress: usize,
    type_parameter_defaults_in_progress: usize,
    mapped_types_in_progress: usize,
    flow_loop_stack: usize,
    flow_loop_start: u32,
    shared_flow: usize,
    tsc_eager_iteration_capture_depth: usize,
    /// Snapshot map (inventory: "snapshot map or forbid across
    /// speculation") — entries are strictly scoped to an in-progress
    /// ReduceLabel arm, so this is empty except when a trial opens
    /// inside a try/finally flow walk.
    reduce_label_overrides: HashMap<(usize, FlowId), Vec<FlowId>>,
    /// Must be empty across the boundary (inventory row); the begin
    /// debug_assert documents that claim, the clone keeps release
    /// builds restoring rather than trusting it.
    exhaustive_switch_computing: HashSet<NodeId>,

    // ---- B: counters / flags ----
    instantiation_depth: u32,
    inline_level: u32,
    in_variance_computation: bool,
    variance_type_parameter: Option<TypeId>,
    /// tsc consumes the did-you-mean budget only on reporting paths;
    /// the port's eager lazy-diagnostic identity lets a trial consume
    /// it, so the transaction gives it back on rollback.
    suggestion_count: u32,
    is_inference_partially_blocked: bool,

    // ---- D: diagnostics sinks ----
    diagnostics: usize,
    visible_global_diagnostics: usize,
    /// Marks into the completed-diagnostic journals. Entries above a
    /// nested checkpoint are promoted to its parent on either commit
    /// or rollback; only the outermost resolution clears the journals.
    tsc_eager_diagnostics: usize,
    tsc_eager_visible_global_diagnostics: usize,
    /// Unlike the tsc-eager iteration journals above, entries above these
    /// marks survive only a commit or a completed-candidate `Reject`.
    completed_contextual_diagnostics: usize,
    completed_contextual_visible_global_diagnostics: usize,
    partial_check_records: usize,
    /// Per-file range-vector lengths; files absent here were inserted
    /// by the trial and are removed wholesale on rollback. A
    /// speculative containment permanently marks a range (affects the
    /// 2578 @ts-expect-error exemption) — the inventory's must-roll-back
    /// item.
    partially_checked_ranges: Vec<(usize, usize)>,
    elaborated_satisfies_expressions: HashSet<NodeId>,
    potential_this_collisions: usize,
    potential_new_target_collisions: usize,
    potential_weak_map_set_collisions: usize,
    potential_reflect_collisions: usize,
    potential_unused_renamed_binding_elements_in_types: usize,
}

impl Drop for SpeculationCheckpoint {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        if !self.resolved && !std::thread::panicking() {
            panic!(
                "SpeculationCheckpoint dropped without commit_speculation \
                 or rollback_speculation (speculation_depth {} leaked)",
                self.depth
            );
        }
    }
}

impl CheckerState<'_> {
    /// tsrs-native: begin an owned journal interval for tsc's eager
    /// reporting-mode iteration diagnostics.
    pub(crate) fn begin_tsc_eager_iteration_diagnostic_capture(
        &mut self,
    ) -> TscEagerIterationCapture {
        if self.speculation_depth == 0 {
            return TscEagerIterationCapture {
                active: false,
                depth: 0,
                sink_marks: None,
                journal_marks: None,
            };
        }
        let is_outermost = self.tsc_eager_iteration_capture_depth == 0;
        self.tsc_eager_iteration_capture_depth += 1;
        TscEagerIterationCapture {
            active: true,
            depth: self.tsc_eager_iteration_capture_depth,
            sink_marks: Some((
                self.diagnostics.len(),
                self.visible_global_diagnostics.len(),
            )),
            journal_marks: is_outermost.then_some((
                self.tsc_eager_diagnostics.len(),
                self.tsc_eager_visible_global_diagnostics.len(),
            )),
        }
    }

    /// tsrs-native: close and reconcile an owned eager-diagnostic journal.
    pub(crate) fn end_tsc_eager_iteration_diagnostic_capture(
        &mut self,
        capture: TscEagerIterationCapture,
    ) {
        if !capture.active {
            return;
        }
        assert_eq!(
            self.tsc_eager_iteration_capture_depth, capture.depth,
            "iteration diagnostic captures must resolve LIFO"
        );
        self.tsc_eager_iteration_capture_depth -= 1;
        let (diagnostics_start, visible_global_diagnostics_start) = capture
            .sink_marks
            .expect("active iteration captures carry sink marks");
        if let Some((diagnostics_journal_start, visible_global_diagnostics_journal_start)) =
            capture.journal_marks
        {
            self.tsc_eager_diagnostics
                .truncate(diagnostics_journal_start);
            self.tsc_eager_visible_global_diagnostics
                .truncate(visible_global_diagnostics_journal_start);
        }
        self.record_tsc_eager_iteration_diagnostics_since(
            diagnostics_start,
            visible_global_diagnostics_start,
        );
    }

    /// Record completed diagnostics emitted by one reporting-mode
    /// iteration entry. Capturing at every completed entry prevents a
    /// nested rejected-candidate rollback from erasing its reporting
    /// rows. When the outermost entry completes, it first truncates the
    /// provisional rows recorded since its journal marks and then
    /// records its current sink suffix, including any related
    /// information attached by the enclosing walk.
    ///
    /// Rows outside a nested reporting interval are never selected, so
    /// ordinary failed-candidate diagnostics still roll back.
    /// tsrs-native: copy completed sink rows into the speculation journal.
    pub(crate) fn record_tsc_eager_iteration_diagnostics_since(
        &mut self,
        diagnostics_start: usize,
        visible_global_diagnostics_start: usize,
    ) {
        if self.speculation_depth == 0 {
            return;
        }
        for diagnostic in &self.diagnostics[diagnostics_start..] {
            if !self.tsc_eager_diagnostics.contains(diagnostic) {
                self.tsc_eager_diagnostics.push(diagnostic.clone());
            }
        }
        for diagnostic in &self.visible_global_diagnostics[visible_global_diagnostics_start..] {
            if !self
                .tsc_eager_visible_global_diagnostics
                .contains(diagnostic)
            {
                self.tsc_eager_visible_global_diagnostics
                    .push(diagnostic.clone());
            }
        }
    }

    /// tsrs-native: journals completed contextual-check sink suffixes across
    /// Rust's speculation transaction; tsc has no candidate transaction.
    ///
    /// Keep the diagnostic half of one successfully completed contextual
    /// function check beside its retained `ContextChecked` and contextual
    /// parameter-type writes. tsc has no candidate transaction: once the
    /// check completes, both effects remain observable even when that
    /// overload candidate is rejected. The transaction therefore journals
    /// only this completed sink suffix rather than preserving unrelated
    /// trial diagnostics.
    pub(crate) fn record_completed_contextual_diagnostics_since(
        &mut self,
        diagnostics_start: usize,
        visible_global_diagnostics_start: usize,
    ) {
        if self.speculation_depth == 0 {
            return;
        }
        for diagnostic in &self.diagnostics[diagnostics_start..] {
            if !self.completed_contextual_diagnostics.contains(diagnostic) {
                self.completed_contextual_diagnostics
                    .push(diagnostic.clone());
            }
        }
        for diagnostic in &self.visible_global_diagnostics[visible_global_diagnostics_start..] {
            if !self
                .completed_contextual_visible_global_diagnostics
                .contains(diagnostic)
            {
                self.completed_contextual_visible_global_diagnostics
                    .push(diagnostic.clone());
            }
        }
    }

    /// tsrs-native: the 7.0t transaction open — no tsc counterpart
    /// (module doc: tsc keeps trials clean via checkMode bypasses).
    ///
    /// Open a speculative region: capture the checkpoint and raise
    /// `speculation_depth` (which arms the links/Signature-cache write
    /// asserts). Every begin must reach exactly one of
    /// `commit_speculation` / `rollback_speculation`; prefer the
    /// `speculate` wrapper, which also owns the Err boundary ordering.
    pub fn begin_speculation(&mut self) -> SpeculationCheckpoint {
        debug_assert!(
            self.exhaustive_switch_computing.is_empty(),
            "exhaustive-switch computation may not straddle a speculation boundary (7.0t inventory)"
        );
        if self.speculation_depth == 0 {
            debug_assert!(
                self.tsc_eager_diagnostics.is_empty()
                    && self.tsc_eager_visible_global_diagnostics.is_empty()
                    && self.completed_contextual_diagnostics.is_empty()
                    && self
                        .completed_contextual_visible_global_diagnostics
                        .is_empty(),
                "outermost speculation must start with empty completed-diagnostic journals"
            );
        }
        self.speculation_depth += 1;
        SpeculationCheckpoint {
            depth: self.speculation_depth,
            resolved: false,
            speculative_links: self.links.speculative_marks(),
            speculative_signature_returns: self.speculative_signature_return_mark(),
            resolution_targets: self.resolution_targets.len(),
            resolution_results: self.resolution_results.len(),
            resolution_property_names: self.resolution_property_names.len(),
            resolution_start: self.resolution_start,
            contextual_type_nodes: self.contextual_type_nodes.len(),
            contextual_types: self.contextual_types.len(),
            contextual_is_cache: self.contextual_is_cache.len(),
            contextual_binding_patterns: self.contextual_binding_patterns.len(),
            inference_context_nodes: self.inference_context_nodes.len(),
            inference_contexts: self.inference_contexts.len(),
            awaited_type_stack: self.awaited_type_stack.len(),
            active_type_mappers: self.active_type_mappers.len(),
            active_type_mappers_caches: self.active_type_mappers_caches.len(),
            variance_handler_stack: self.variance_handler_stack.len(),
            class_interface_declared_in_progress: self.class_interface_declared_in_progress.len(),
            type_parameter_defaults_in_progress: self.type_parameter_defaults_in_progress.len(),
            mapped_types_in_progress: self.mapped_types_in_progress.len(),
            flow_loop_stack: self.flow_loop_stack.len(),
            flow_loop_start: self.flow_loop_start,
            shared_flow: self.shared_flow.len(),
            tsc_eager_iteration_capture_depth: self.tsc_eager_iteration_capture_depth,
            reduce_label_overrides: self.reduce_label_overrides.clone(),
            exhaustive_switch_computing: self.exhaustive_switch_computing.clone(),
            instantiation_depth: self.instantiation_depth,
            inline_level: self.inline_level,
            in_variance_computation: self.in_variance_computation,
            variance_type_parameter: self.variance_type_parameter,
            suggestion_count: self.suggestion_count,
            is_inference_partially_blocked: self.is_inference_partially_blocked,
            diagnostics: self.diagnostics.len(),
            visible_global_diagnostics: self.visible_global_diagnostics.len(),
            tsc_eager_diagnostics: self.tsc_eager_diagnostics.len(),
            tsc_eager_visible_global_diagnostics: self.tsc_eager_visible_global_diagnostics.len(),
            completed_contextual_diagnostics: self.completed_contextual_diagnostics.len(),
            completed_contextual_visible_global_diagnostics: self
                .completed_contextual_visible_global_diagnostics
                .len(),
            partial_check_records: self.partial_check_records.len(),
            partially_checked_ranges: self
                .partially_checked_ranges
                .iter()
                .map(|(&file, ranges)| (file, ranges.len()))
                .collect(),
            elaborated_satisfies_expressions: self.elaborated_satisfies_expressions.clone(),
            potential_this_collisions: self.potential_this_collisions.len(),
            potential_new_target_collisions: self.potential_new_target_collisions.len(),
            potential_weak_map_set_collisions: self.potential_weak_map_set_collisions.len(),
            potential_reflect_collisions: self.potential_reflect_collisions.len(),
            potential_unused_renamed_binding_elements_in_types: self
                .potential_unused_renamed_binding_elements_in_types
                .len(),
        }
    }

    /// tsrs-native: the 7.0t transaction commit — no tsc counterpart.
    ///
    /// The trial succeeded: keep everything it produced (diagnostics,
    /// sink pushes, budget consumption) and drop the guard. The
    /// transient stacks must already be balanced — an imbalance here is
    /// a missing pop/revert twin inside the region, the same bug class
    /// check.rs's abort-unwind census catches per element.
    pub fn commit_speculation(&mut self, mut checkpoint: SpeculationCheckpoint) {
        assert_eq!(
            self.speculation_depth, checkpoint.depth,
            "speculation transactions must resolve LIFO"
        );
        debug_assert_eq!(
            self.tsc_eager_iteration_capture_depth, checkpoint.tsc_eager_iteration_capture_depth,
            "a speculation transaction committed with an unbalanced reporting iteration capture"
        );
        self.links
            .commit_speculative_writes(checkpoint.speculative_links, checkpoint.depth - 1);
        self.commit_speculative_signature_returns(
            checkpoint.speculative_signature_returns,
            checkpoint.depth - 1,
        );
        checkpoint.resolved = true;
        self.speculation_depth -= 1;
        if self.speculation_depth == 0 {
            self.tsc_eager_diagnostics.clear();
            self.tsc_eager_visible_global_diagnostics.clear();
            self.completed_contextual_diagnostics.clear();
            self.completed_contextual_visible_global_diagnostics.clear();
        }
        #[cfg(test)]
        {
            self.speculation_commit_count += 1;
        }
        #[cfg(debug_assertions)]
        {
            let balanced = [
                (self.resolution_targets.len(), checkpoint.resolution_targets),
                (self.resolution_results.len(), checkpoint.resolution_results),
                (
                    self.resolution_property_names.len(),
                    checkpoint.resolution_property_names,
                ),
                (self.resolution_start, checkpoint.resolution_start),
                (
                    self.contextual_type_nodes.len(),
                    checkpoint.contextual_type_nodes,
                ),
                (self.contextual_types.len(), checkpoint.contextual_types),
                (
                    self.contextual_is_cache.len(),
                    checkpoint.contextual_is_cache,
                ),
                (
                    self.contextual_binding_patterns.len(),
                    checkpoint.contextual_binding_patterns,
                ),
                (
                    self.inference_context_nodes.len(),
                    checkpoint.inference_context_nodes,
                ),
                (self.inference_contexts.len(), checkpoint.inference_contexts),
                (self.awaited_type_stack.len(), checkpoint.awaited_type_stack),
                (
                    self.active_type_mappers.len(),
                    checkpoint.active_type_mappers,
                ),
                (
                    self.active_type_mappers_caches.len(),
                    checkpoint.active_type_mappers_caches,
                ),
                (
                    self.variance_handler_stack.len(),
                    checkpoint.variance_handler_stack,
                ),
                (
                    self.class_interface_declared_in_progress.len(),
                    checkpoint.class_interface_declared_in_progress,
                ),
                (
                    self.type_parameter_defaults_in_progress.len(),
                    checkpoint.type_parameter_defaults_in_progress,
                ),
                (
                    self.mapped_types_in_progress.len(),
                    checkpoint.mapped_types_in_progress,
                ),
                (self.flow_loop_stack.len(), checkpoint.flow_loop_stack),
                (
                    self.flow_loop_start as usize,
                    checkpoint.flow_loop_start as usize,
                ),
                (self.shared_flow.len(), checkpoint.shared_flow),
                (
                    self.tsc_eager_iteration_capture_depth,
                    checkpoint.tsc_eager_iteration_capture_depth,
                ),
                (
                    self.instantiation_depth as usize,
                    checkpoint.instantiation_depth as usize,
                ),
                (self.inline_level as usize, checkpoint.inline_level as usize),
            ];
            for (index, (now, at_begin)) in balanced.iter().enumerate() {
                assert_eq!(
                    now, at_begin,
                    "speculative region committed with unbalanced transient state (slot {index})"
                );
            }
            assert_eq!(
                self.in_variance_computation, checkpoint.in_variance_computation,
                "speculative region committed with unbalanced variance flag"
            );
            assert_eq!(
                self.variance_type_parameter, checkpoint.variance_type_parameter,
                "speculative region committed with unbalanced variance type parameter"
            );
            assert_eq!(
                self.is_inference_partially_blocked, checkpoint.is_inference_partially_blocked,
                "speculative region committed with unbalanced inference-blocked flag"
            );
            assert_eq!(
                self.reduce_label_overrides, checkpoint.reduce_label_overrides,
                "speculative region committed with unbalanced ReduceLabel overrides"
            );
            assert!(
                self.exhaustive_switch_computing.is_empty(),
                "speculative region committed inside an exhaustive-switch computation"
            );
        }
    }

    /// tsrs-native: the 7.0t transaction rollback — no tsc
    /// counterpart.
    ///
    /// The caller explicitly discarded the region (or it aborted): restore
    /// every A/B/D inventory item and every journaled category-C publication
    /// to the checkpoint. Completed overload rejection uses the distinct
    /// `Reject` path above. `instantiation_count`, `deferred_nodes`,
    /// `assertion_expression_type`, and the `flow_analysis_disabled`
    /// latch deliberately survive: tsc never unwinds those semantic
    /// registrations, and deferred assertion checking consumes its
    /// stashed operand type after the candidate boundary.
    pub fn rollback_speculation(&mut self, checkpoint: SpeculationCheckpoint) {
        self.finish_speculation_unwind(checkpoint, false);
    }

    /// Close a completed, rejected overload candidate. Unlike an abort or
    /// an explicit rollback, tsc retains completed declaration signatures,
    /// signature return types, contextual-function once-state, and the
    /// diagnostics emitted while completing that once-state for the next
    /// candidate/relation pass.
    fn reject_speculation(&mut self, checkpoint: SpeculationCheckpoint) {
        self.finish_speculation_unwind(checkpoint, true);
    }

    fn finish_speculation_unwind(
        &mut self,
        mut checkpoint: SpeculationCheckpoint,
        retain_completed_semantics: bool,
    ) {
        assert_eq!(
            self.speculation_depth, checkpoint.depth,
            "speculation transactions must resolve LIFO"
        );
        debug_assert_eq!(
            self.tsc_eager_iteration_capture_depth, checkpoint.tsc_eager_iteration_capture_depth,
            "a speculation transaction rolled back with an unbalanced reporting iteration capture"
        );
        let tsc_eager_diagnostics =
            self.tsc_eager_diagnostics[checkpoint.tsc_eager_diagnostics..].to_vec();
        let tsc_eager_visible_global_diagnostics = self.tsc_eager_visible_global_diagnostics
            [checkpoint.tsc_eager_visible_global_diagnostics..]
            .to_vec();
        let completed_contextual_diagnostics = retain_completed_semantics.then(|| {
            self.completed_contextual_diagnostics[checkpoint.completed_contextual_diagnostics..]
                .to_vec()
        });
        let completed_contextual_visible_global_diagnostics =
            retain_completed_semantics.then(|| {
                self.completed_contextual_visible_global_diagnostics
                    [checkpoint.completed_contextual_visible_global_diagnostics..]
                    .to_vec()
            });
        if retain_completed_semantics {
            self.links
                .commit_speculative_writes(checkpoint.speculative_links, checkpoint.depth - 1);
            self.commit_speculative_signature_returns(
                checkpoint.speculative_signature_returns,
                checkpoint.depth - 1,
            );
        } else {
            self.links
                .restore_speculative_writes(checkpoint.speculative_links);
            self.restore_speculative_signature_returns(checkpoint.speculative_signature_returns);
        }
        checkpoint.resolved = true;
        self.speculation_depth -= 1;
        #[cfg(test)]
        {
            self.speculation_rollback_count += 1;
        }

        // A: transient stacks.
        self.resolution_targets
            .truncate(checkpoint.resolution_targets);
        self.resolution_results
            .truncate(checkpoint.resolution_results);
        self.resolution_property_names
            .truncate(checkpoint.resolution_property_names);
        self.resolution_start = checkpoint.resolution_start;
        self.contextual_type_nodes
            .truncate(checkpoint.contextual_type_nodes);
        self.contextual_types.truncate(checkpoint.contextual_types);
        self.contextual_is_cache
            .truncate(checkpoint.contextual_is_cache);
        self.contextual_binding_patterns
            .truncate(checkpoint.contextual_binding_patterns);
        self.inference_context_nodes
            .truncate(checkpoint.inference_context_nodes);
        self.inference_contexts
            .truncate(checkpoint.inference_contexts);
        self.awaited_type_stack
            .truncate(checkpoint.awaited_type_stack);
        self.active_type_mappers
            .truncate(checkpoint.active_type_mappers);
        self.active_type_mappers_caches
            .truncate(checkpoint.active_type_mappers_caches);
        self.variance_handler_stack
            .truncate(checkpoint.variance_handler_stack);
        self.class_interface_declared_in_progress
            .truncate(checkpoint.class_interface_declared_in_progress);
        self.type_parameter_defaults_in_progress
            .truncate(checkpoint.type_parameter_defaults_in_progress);
        self.mapped_types_in_progress
            .truncate(checkpoint.mapped_types_in_progress);
        self.flow_loop_stack.truncate(checkpoint.flow_loop_stack);
        self.flow_loop_start = checkpoint.flow_loop_start;
        self.shared_flow.truncate(checkpoint.shared_flow);
        self.tsc_eager_iteration_capture_depth = checkpoint.tsc_eager_iteration_capture_depth;
        self.reduce_label_overrides = std::mem::take(&mut checkpoint.reduce_label_overrides);
        self.exhaustive_switch_computing =
            std::mem::take(&mut checkpoint.exhaustive_switch_computing);

        // B: counters / flags.
        self.instantiation_depth = checkpoint.instantiation_depth;
        self.inline_level = checkpoint.inline_level;
        self.in_variance_computation = checkpoint.in_variance_computation;
        self.variance_type_parameter = checkpoint.variance_type_parameter;
        self.suggestion_count = checkpoint.suggestion_count;
        self.is_inference_partially_blocked = checkpoint.is_inference_partially_blocked;

        // D: diagnostics sinks.
        self.diagnostics.truncate(checkpoint.diagnostics);
        self.visible_global_diagnostics
            .truncate(checkpoint.visible_global_diagnostics);
        for diagnostic in tsc_eager_diagnostics {
            self.push_error_diagnostic(diagnostic);
        }
        for diagnostic in tsc_eager_visible_global_diagnostics {
            if !self.visible_global_diagnostics.contains(&diagnostic) {
                self.visible_global_diagnostics.push(diagnostic);
            }
        }
        if let Some(diagnostics) = completed_contextual_diagnostics {
            for diagnostic in diagnostics {
                self.push_error_diagnostic(diagnostic);
            }
        } else {
            self.completed_contextual_diagnostics
                .truncate(checkpoint.completed_contextual_diagnostics);
        }
        if let Some(diagnostics) = completed_contextual_visible_global_diagnostics {
            for diagnostic in diagnostics {
                if !self.visible_global_diagnostics.contains(&diagnostic) {
                    self.visible_global_diagnostics.push(diagnostic);
                }
            }
        } else {
            self.completed_contextual_visible_global_diagnostics
                .truncate(checkpoint.completed_contextual_visible_global_diagnostics);
        }
        // Nested resolution promotes its completed iteration rows to
        // the parent by leaving both journals intact. The outermost
        // boundary has replayed their final copies and can release the
        // temporary storage.
        if self.speculation_depth == 0 {
            self.tsc_eager_diagnostics.clear();
            self.tsc_eager_visible_global_diagnostics.clear();
            self.completed_contextual_diagnostics.clear();
            self.completed_contextual_visible_global_diagnostics.clear();
        }
        self.partial_check_records
            .truncate(checkpoint.partial_check_records);
        let saved_ranges: HashMap<usize, usize> = checkpoint
            .partially_checked_ranges
            .iter()
            .copied()
            .collect();
        self.partially_checked_ranges
            .retain(|file, ranges| match saved_ranges.get(file) {
                Some(&length) => {
                    ranges.truncate(length);
                    true
                }
                None => false,
            });
        self.elaborated_satisfies_expressions =
            std::mem::take(&mut checkpoint.elaborated_satisfies_expressions);
        self.potential_this_collisions
            .truncate(checkpoint.potential_this_collisions);
        self.potential_new_target_collisions
            .truncate(checkpoint.potential_new_target_collisions);
        self.potential_weak_map_set_collisions
            .truncate(checkpoint.potential_weak_map_set_collisions);
        self.potential_reflect_collisions
            .truncate(checkpoint.potential_reflect_collisions);
        self.potential_unused_renamed_binding_elements_in_types
            .truncate(checkpoint.potential_unused_renamed_binding_elements_in_types);
    }

    /// tsrs-native: the 7.0t scoped-transaction wrapper — no tsc
    /// counterpart.
    ///
    /// Run `f` inside a speculation transaction. The closure's
    /// `SpeculationOutcome` selects commit, completed-candidate rejection,
    /// or full rollback; an
    /// `Err(CheckAbort)` ALWAYS rolls back, and does so BEFORE the Err
    /// re-propagates — outer Err-revert twins therefore fire with
    /// `speculation_depth` already restored (the boundary ordering
    /// rule, module doc).
    pub fn speculate<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> CheckResult<SpeculationOutcome<T>>,
    ) -> CheckResult<T> {
        let checkpoint = self.begin_speculation();
        match f(self) {
            Ok(SpeculationOutcome::Commit(value)) => {
                self.commit_speculation(checkpoint);
                Ok(value)
            }
            Ok(SpeculationOutcome::Reject(value)) => {
                self.reject_speculation(checkpoint);
                Ok(value)
            }
            Ok(SpeculationOutcome::Rollback(value)) => {
                self.rollback_speculation(checkpoint);
                Ok(value)
            }
            Err(abort) => {
                self.rollback_speculation(checkpoint);
                Err(abort)
            }
        }
    }
}

// The failed-candidate rollback tests the START PRECONDITION names:
// every inventory category is mutated inside a transaction and the
// rollback/commit/Err paths are checked against the checkpoint,
// including the deliberate non-restorations (instantiation_count,
// deferred_nodes) and the boundary ordering rule.
#[cfg(test)]
#[path = "../tests/unit/speculate/tests.rs"]
mod tests;
