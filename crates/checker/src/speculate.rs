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
//!   `is_inference_partially_blocked` restore on rollback.
//!   `instantiation_count` deliberately does NOT restore —
//!   tsc resets it at the three check entry points (86551/86921/80965)
//!   and never mid-resolution. `flow_analysis_disabled` is a one-way
//!   latch even in tsc — left alone.
//! - C (permanent-truth caches, links, interners): completed results
//!   persist at either boundary. Call resolvedSignature slots still
//!   journal their entry state for cleanup of unresolved sentinels;
//!   type-only alias declarations still restore their entry values.
//!   Fresh semantic objects and structurally keyed type interners remain
//!   monotone and candidate-independent, as in tsc.
//! - D (diagnostics sinks): diagnostics and partial-check records persist
//!   at either boundary. `deferred_nodes` deliberately survives rollback
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

use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

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
    reliability_flags: tsc_types::RelationComparisonResult,
    type_parameter_defaults_in_progress: usize,
    mapped_types_in_progress: usize,
    flow_loop_stack: usize,
    flow_loop_start: u32,
    shared_flow: usize,
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
    variance_stack: usize,
    variance_type_parameter: Option<TypeId>,
    is_inference_partially_blocked: bool,

    // ---- D: diagnostics sinks ----
    elaborated_satisfies_expressions: HashSet<NodeId>,
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
    /// tsrs-native: the 7.0t transaction open — no tsc counterpart
    /// (module doc: tsc keeps trials clean via checkMode bypasses).
    ///
    /// Open a speculative region: capture the checkpoint and raise
    /// `speculation_depth` (which arms the links/Signature-cache write
    /// asserts). Every begin must reach exactly one of
    /// `commit_speculation` / `rollback_speculation`; prefer the
    /// `speculate` wrapper, which also owns the Err boundary ordering.
    pub fn begin_speculation(&mut self) -> SpeculationCheckpoint {
        // `exhaustive_switch_computing` is a category-A transient: the
        // checkpoint clones it, commit requires it balanced and rollback
        // restores it. The 7.0t inventory assumed it empty at every
        // boundary, but computeExhaustiveSwitchStatement checks the switch
        // expression (checkExpressionCached, _tsc.js:78946) and the clause
        // expressions, which resolve calls and open candidate trials
        // (VS Code `src` witness: `switch (f(x))` reached from an implicit-
        // return query while its function's return type is inferred).
        self.speculation_depth += 1;
        SpeculationCheckpoint {
            depth: self.speculation_depth,
            resolved: false,
            speculative_links: self.links.speculative_marks(),
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
            reliability_flags: self.reliability_flags,
            type_parameter_defaults_in_progress: self.type_parameter_defaults_in_progress.len(),
            mapped_types_in_progress: self.mapped_types_in_progress.len(),
            flow_loop_stack: self.flow_loop_stack.len(),
            flow_loop_start: self.flow_loop_start,
            shared_flow: self.shared_flow.len(),
            reduce_label_overrides: self.reduce_label_overrides.clone(),
            exhaustive_switch_computing: self.exhaustive_switch_computing.clone(),
            instantiation_depth: self.instantiation_stack.len() as u32,
            inline_level: self.inline_level,
            variance_stack: self.variance_stack.len(),
            variance_type_parameter: self.variance_type_parameter,
            is_inference_partially_blocked: self.is_inference_partially_blocked,
            elaborated_satisfies_expressions: self.elaborated_satisfies_expressions.clone(),
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
    /// sink pushes) and drop the guard. The transient stacks must
    /// already be balanced — an imbalance here is a missing pop/revert
    /// twin inside the region, the same bug class check.rs's
    /// abort-unwind census catches per element.
    pub fn commit_speculation(&mut self, mut checkpoint: SpeculationCheckpoint) {
        assert_eq!(
            self.speculation_depth, checkpoint.depth,
            "speculation transactions must resolve LIFO"
        );
        self.links
            .commit_speculative_writes(checkpoint.speculative_links, checkpoint.depth - 1);
        checkpoint.resolved = true;
        self.speculation_depth -= 1;
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
                    self.instantiation_stack.len(),
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
                self.variance_stack.len(),
                checkpoint.variance_stack,
                "speculative region committed with unbalanced variance stack"
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
            assert_eq!(
                self.exhaustive_switch_computing, checkpoint.exhaustive_switch_computing,
                "speculative region committed with unbalanced exhaustive-switch computations"
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
        if retain_completed_semantics {
            self.links
                .commit_speculative_writes(checkpoint.speculative_links, checkpoint.depth - 1);
        } else {
            self.links
                .restore_speculative_writes(checkpoint.speculative_links);
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
        self.reliability_flags = checkpoint.reliability_flags;
        self.type_parameter_defaults_in_progress
            .truncate(checkpoint.type_parameter_defaults_in_progress);
        self.mapped_types_in_progress
            .truncate(checkpoint.mapped_types_in_progress);
        self.flow_loop_stack.truncate(checkpoint.flow_loop_stack);
        self.flow_loop_start = checkpoint.flow_loop_start;
        self.shared_flow.truncate(checkpoint.shared_flow);
        self.reduce_label_overrides = std::mem::take(&mut checkpoint.reduce_label_overrides);
        self.exhaustive_switch_computing =
            std::mem::take(&mut checkpoint.exhaustive_switch_computing);

        // B: counters / flags.
        self.instantiation_stack
            .truncate(checkpoint.instantiation_depth as usize);
        self.inline_level = checkpoint.inline_level;
        self.variance_stack.truncate(checkpoint.variance_stack);
        self.variance_type_parameter = checkpoint.variance_type_parameter;
        self.is_inference_partially_blocked = checkpoint.is_inference_partially_blocked;

        // D: diagnostic sinks and partial-check records persist with the
        // completed node results they describe.
        self.elaborated_satisfies_expressions =
            std::mem::take(&mut checkpoint.elaborated_satisfies_expressions);
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
