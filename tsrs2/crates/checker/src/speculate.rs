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
//! needs the explicit transaction on top because (a) an Unsupported
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
//! - C (permanent-truth caches, links, interners): NEVER rolled back.
//!   Values are candidate-independent by the same construction tsc
//!   relies on (structural keying + the checkMode bypasses); entries
//!   minted during a failed trial are garbage, not poison. The
//!   speculation_depth assert net on links/Signature-cache writes stays
//!   in force as the 7.4 wiring inventory: each site a live trial
//!   exercises gets a per-site, evidence-backed relaxation THERE, not a
//!   blanket one here. 7.5d addition to that inventory: B8 put erased
//!   AND canonical signature computation on EVERY relation compare
//!   (signatureRelatedTo erase honoring + the generic-source arm), so
//!   when trials gain a producer, the first-touch cache writes in
//!   get_erased_signature / get_canonical_signature /
//!   get_base_signature and the optional-call cache are the sites an
//!   in-trial relation check hits first — wire their relaxations
//!   before flipping trials live.
//! - D (diagnostics sinks): truncated to the checkpoint marks on
//!   rollback (push-dedupe is order-safe under truncation), kept on
//!   commit. `deferred_nodes` deliberately survives rollback — tsc
//!   checkNodeDeferred (86899-86908) registers unconditionally, and
//!   deferred nodes registered under a failed candidate are still
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

use tsrs2_binder::flow::FlowId;
use tsrs2_syntax::NodeId;
use tsrs2_types::TypeId;

use crate::state::{CheckResult2, CheckerState};

/// What a completed speculative region wants done with the state it
/// accumulated: `Commit` keeps it (the candidate succeeded), `Rollback`
/// restores the checkpoint (the candidate failed but resolution
/// continues — chooseOverload moves to the next candidate).
pub enum SpeculationOutcome<T> {
    Commit(T),
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
        self.speculation_depth += 1;
        SpeculationCheckpoint {
            depth: self.speculation_depth,
            resolved: false,
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
            flow_loop_stack: self.flow_loop_stack.len(),
            flow_loop_start: self.flow_loop_start,
            shared_flow: self.shared_flow.len(),
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
    /// check.rs's unsupported-unwind census catches per element.
    pub fn commit_speculation(&mut self, mut checkpoint: SpeculationCheckpoint) {
        assert_eq!(
            self.speculation_depth, checkpoint.depth,
            "speculation transactions must resolve LIFO"
        );
        checkpoint.resolved = true;
        self.speculation_depth -= 1;
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
                (self.flow_loop_stack.len(), checkpoint.flow_loop_stack),
                (
                    self.flow_loop_start as usize,
                    checkpoint.flow_loop_start as usize,
                ),
                (self.shared_flow.len(), checkpoint.shared_flow),
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
    /// The trial failed (or aborted): restore every A/B/D inventory
    /// item to the checkpoint. Permanent-truth caches (category C),
    /// `instantiation_count`, `deferred_nodes`, and the
    /// `flow_analysis_disabled` latch deliberately survive — see the
    /// module doc.
    pub fn rollback_speculation(&mut self, mut checkpoint: SpeculationCheckpoint) {
        assert_eq!(
            self.speculation_depth, checkpoint.depth,
            "speculation transactions must resolve LIFO"
        );
        checkpoint.resolved = true;
        self.speculation_depth -= 1;

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
        self.flow_loop_stack.truncate(checkpoint.flow_loop_stack);
        self.flow_loop_start = checkpoint.flow_loop_start;
        self.shared_flow.truncate(checkpoint.shared_flow);
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
    /// `SpeculationOutcome` decides commit vs rollback; an
    /// `Err(Unsupported)` ALWAYS rolls back, and does so BEFORE the Err
    /// re-propagates — outer Err-revert twins therefore fire with
    /// `speculation_depth` already restored (the boundary ordering
    /// rule, module doc).
    pub fn speculate<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> CheckResult2<SpeculationOutcome<T>>,
    ) -> CheckResult2<T> {
        let checkpoint = self.begin_speculation();
        match f(self) {
            Ok(SpeculationOutcome::Commit(value)) => {
                self.commit_speculation(checkpoint);
                Ok(value)
            }
            Ok(SpeculationOutcome::Rollback(value)) => {
                self.rollback_speculation(checkpoint);
                Ok(value)
            }
            Err(unsupported) => {
                self.rollback_speculation(checkpoint);
                Err(unsupported)
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
mod tests {
    use tsrs2_binder::flow::FlowId;
    use tsrs2_diags::gen as diagnostics;
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeSystemPropertyName};

    use super::SpeculationOutcome;
    use crate::flow::FlowType;
    use crate::state::test_support::with_program_state;
    use crate::state::{CheckerState, SignatureKind, Unsupported};

    fn with_state<R>(run: impl FnOnce(&mut CheckerState) -> R) -> R {
        with_program_state(
            &[("a.ts", "declare var a: string;\n")],
            &CompilerOptions::default(),
            run,
        )
    }

    /// Push something onto every checkpoint-covered piece of state a
    /// unit test can reach without heavier machinery, then return the
    /// values needed to assert restoration.
    fn mutate_everything(state: &mut CheckerState) {
        let root = state.binder.source(0).root;
        let string = state.tables.intrinsics.string;
        // A: transient stacks.
        state.resolution_results.push(true);
        state
            .resolution_property_names
            .push(TypeSystemPropertyName::TYPE);
        state.resolution_start += 7;
        state.contextual_type_nodes.push(root);
        state.contextual_types.push(Some(string));
        state.contextual_is_cache.push(false);
        state.contextual_binding_patterns.push(root);
        state.inference_context_nodes.push(root);
        state.inference_contexts.push(None);
        state.awaited_type_stack.push(string);
        state
            .active_type_mappers_caches
            .push(std::collections::HashMap::new());
        state.flow_loop_start += 3;
        state.shared_flow.push((FlowId(0), FlowType::Type(string)));
        state
            .reduce_label_overrides
            .insert((0, FlowId(1)), vec![FlowId(2)]);
        state.exhaustive_switch_computing.insert(root);
        // B: counters / flags.
        state.instantiation_depth += 5;
        state.inline_level += 2;
        state.in_variance_computation = true;
        state.variance_type_parameter = Some(string);
        state.suggestion_count += 4;
        state.is_inference_partially_blocked = true;
        // D: diagnostics sinks.
        let diagnostic = state.create_error(None, &diagnostics::Cannot_find_name_0, &["x"]);
        state.visible_global_diagnostics.push(diagnostic.clone());
        state.push_error_diagnostic(diagnostic);
        state.mark_partially_checked_node(root, "7.0t rollback test");
        state.elaborated_satisfies_expressions.insert(root);
        state.potential_this_collisions.push(root);
        state.potential_new_target_collisions.push(root);
        state.potential_weak_map_set_collisions.push(root);
        state.potential_reflect_collisions.push(root);
        state
            .potential_unused_renamed_binding_elements_in_types
            .push(root);
    }

    /// The observable projection of every checkpoint-covered field —
    /// captured before begin and compared after rollback (the state
    /// constructor itself emits lib-less global diagnostics, so
    /// absolute zeros are wrong; restoration-to-baseline is the
    /// contract).
    #[derive(Debug, PartialEq)]
    struct Observed {
        speculation_depth: u32,
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
        active_type_mappers_caches: usize,
        flow_loop_start: u32,
        shared_flow: usize,
        reduce_label_overrides: usize,
        exhaustive_switch_computing: usize,
        instantiation_depth: u32,
        inline_level: u32,
        in_variance_computation: bool,
        variance_type_parameter: Option<tsrs2_types::TypeId>,
        suggestion_count: u32,
        is_inference_partially_blocked: bool,
        diagnostics: usize,
        visible_global_diagnostics: usize,
        partial_check_records: usize,
        partially_checked_files: usize,
        elaborated_satisfies_expressions: usize,
        potential_this_collisions: usize,
        potential_new_target_collisions: usize,
        potential_weak_map_set_collisions: usize,
        potential_reflect_collisions: usize,
        potential_unused_renamed_binding_elements_in_types: usize,
    }

    fn observe(state: &CheckerState) -> Observed {
        Observed {
            speculation_depth: state.speculation_depth,
            resolution_results: state.resolution_results.len(),
            resolution_property_names: state.resolution_property_names.len(),
            resolution_start: state.resolution_start,
            contextual_type_nodes: state.contextual_type_nodes.len(),
            contextual_types: state.contextual_types.len(),
            contextual_is_cache: state.contextual_is_cache.len(),
            contextual_binding_patterns: state.contextual_binding_patterns.len(),
            inference_context_nodes: state.inference_context_nodes.len(),
            inference_contexts: state.inference_contexts.len(),
            awaited_type_stack: state.awaited_type_stack.len(),
            active_type_mappers_caches: state.active_type_mappers_caches.len(),
            flow_loop_start: state.flow_loop_start,
            shared_flow: state.shared_flow.len(),
            reduce_label_overrides: state.reduce_label_overrides.len(),
            exhaustive_switch_computing: state.exhaustive_switch_computing.len(),
            instantiation_depth: state.instantiation_depth,
            inline_level: state.inline_level,
            in_variance_computation: state.in_variance_computation,
            variance_type_parameter: state.variance_type_parameter,
            suggestion_count: state.suggestion_count,
            is_inference_partially_blocked: state.is_inference_partially_blocked,
            diagnostics: state.diagnostics.len(),
            visible_global_diagnostics: state.visible_global_diagnostics.len(),
            partial_check_records: state.partial_check_records.len(),
            partially_checked_files: state.partially_checked_ranges.len(),
            elaborated_satisfies_expressions: state.elaborated_satisfies_expressions.len(),
            potential_this_collisions: state.potential_this_collisions.len(),
            potential_new_target_collisions: state.potential_new_target_collisions.len(),
            potential_weak_map_set_collisions: state.potential_weak_map_set_collisions.len(),
            potential_reflect_collisions: state.potential_reflect_collisions.len(),
            potential_unused_renamed_binding_elements_in_types: state
                .potential_unused_renamed_binding_elements_in_types
                .len(),
        }
    }

    #[test]
    fn rollback_restores_stacks_counters_and_sinks() {
        with_state(|state| {
            let before = observe(state);
            let checkpoint = state.begin_speculation();
            assert_eq!(state.speculation_depth, 1);
            mutate_everything(state);
            state.rollback_speculation(checkpoint);
            assert_eq!(observe(state), before);
        });
    }

    #[test]
    fn rollback_keeps_instantiation_count_and_deferred_nodes() {
        with_state(|state| {
            let root = state.binder.source(0).root;
            let checkpoint = state.begin_speculation();
            // instantiation_count is monotone per element in tsc —
            // reset only at the three check entry points, never on a
            // failed candidate.
            state.instantiation_count += 11;
            // tsc checkNodeDeferred (86899) registers unconditionally;
            // nodes deferred under a failed candidate are still
            // checked.
            state.deferred_nodes.entry(root).or_default().insert(root);
            state.rollback_speculation(checkpoint);
            assert_eq!(state.instantiation_count, 11);
            assert!(state.deferred_nodes.contains_key(&root));
        });
    }

    #[test]
    fn commit_keeps_sinks_and_budget_consumption() {
        with_state(|state| {
            let root = state.binder.source(0).root;
            let before = observe(state);
            let checkpoint = state.begin_speculation();
            let diagnostic = state.create_error(None, &diagnostics::Cannot_find_name_0, &["x"]);
            state.push_error_diagnostic(diagnostic);
            state.mark_partially_checked_node(root, "7.0t commit test");
            state.suggestion_count += 1;
            state.commit_speculation(checkpoint);
            assert_eq!(state.speculation_depth, 0);
            assert_eq!(state.diagnostics.len(), before.diagnostics + 1);
            assert_eq!(
                state.partial_check_records.len(),
                before.partial_check_records + 1
            );
            assert_eq!(state.suggestion_count, before.suggestion_count + 1);
        });
    }

    #[test]
    fn speculate_outcomes_commit_and_rollback() {
        with_state(|state| {
            let baseline = state.diagnostics.len();
            let committed = state.speculate(|state| {
                let diagnostic =
                    state.create_error(None, &diagnostics::Cannot_find_name_0, &["kept"]);
                state.push_error_diagnostic(diagnostic);
                Ok(SpeculationOutcome::Commit(1))
            });
            assert_eq!(committed, Ok(1));
            assert_eq!(state.diagnostics.len(), baseline + 1);

            let rolled_back = state.speculate(|state| {
                let diagnostic =
                    state.create_error(None, &diagnostics::Cannot_find_name_0, &["dropped"]);
                state.push_error_diagnostic(diagnostic);
                Ok(SpeculationOutcome::Rollback(2))
            });
            assert_eq!(rolled_back, Ok(2));
            assert_eq!(state.diagnostics.len(), baseline + 1);
            assert_eq!(state.speculation_depth, 0);
        });
    }

    /// The boundary ordering rule: by the time the caller sees the
    /// Err, the rollback has already happened — outer Err-revert twins
    /// run at the entry depth.
    #[test]
    fn speculate_rolls_back_before_err_reaches_caller() {
        with_state(|state| {
            // NOT a containment escape — a synthetic Err exercising the
            // transaction boundary. Struct-literal construction keeps
            // the escapes manifest (which scans `Unsupported::new`
            // call sites) tracking real containment debt only.
            let boundary_probe = || Unsupported {
                reason: "7.0t boundary test".to_owned(),
            };
            let before = observe(state);
            let result: Result<(), _> = state.speculate(|state| {
                assert_eq!(state.speculation_depth, 1);
                mutate_everything(state);
                Err(boundary_probe())
            });
            assert_eq!(result, Err(boundary_probe()));
            assert_eq!(observe(state), before);
        });
    }

    #[test]
    fn nested_speculation_resolves_lifo() {
        with_state(|state| {
            let string = state.tables.intrinsics.string;
            let outer = state.begin_speculation();
            state.awaited_type_stack.push(string);
            let inner = state.begin_speculation();
            assert_eq!(state.speculation_depth, 2);
            state.awaited_type_stack.push(string);
            state.rollback_speculation(inner);
            assert_eq!(state.speculation_depth, 1);
            assert_eq!(state.awaited_type_stack.len(), 1);
            state.rollback_speculation(outer);
            assert_eq!(state.speculation_depth, 0);
            assert_eq!(state.awaited_type_stack.len(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "LIFO")]
    fn out_of_order_resolution_panics() {
        with_state(|state| {
            let outer = state.begin_speculation();
            let inner = state.begin_speculation();
            state.rollback_speculation(outer);
            // Unreachable; silence the must-resolve drop guard on the
            // inner checkpoint if the panic above ever regresses.
            state.rollback_speculation(inner);
        });
    }

    #[test]
    #[should_panic(expected = "dropped without commit_speculation")]
    fn unresolved_checkpoint_drop_panics() {
        with_state(|state| {
            let checkpoint = state.begin_speculation();
            drop(checkpoint);
        });
    }

    /// The B35 convention: revert twins restore state and are legal at
    /// any depth (before 7.0t this panicked via assert_writable).
    #[test]
    fn revert_twin_is_legal_under_speculation() {
        with_state(|state| {
            let root = state.binder.source(0).root;
            state.links.set_node_enum_values_computed(0, root);
            state.speculation_depth = 1;
            state.links.revert_node_enum_values_computed(root);
            state.speculation_depth = 0;
            assert!(!state.links.node(root).enum_values_computed);
        });
    }

    #[test]
    fn ranges_rollback_truncates_files_and_removes_new_ones() {
        with_program_state(
            &[
                ("a.ts", "declare var a: string;\n"),
                ("b.ts", "declare var b: string;\n"),
            ],
            &CompilerOptions::default(),
            |state| {
                let file_a = state.binder.source(0).root;
                let file_b = state.binder.source(1).root;
                state.mark_partially_checked_node(file_a, "pre-existing");
                let records_before = state.partial_check_records.len();
                let checkpoint = state.begin_speculation();
                // Same file: appends to the existing range vector.
                let statement_a = state.binder.source(0).arena.node_ids().nth(1);
                if let Some(node) = statement_a {
                    state.mark_partially_checked_node(node, "speculative same-file");
                }
                // New file: inserts a fresh map key.
                state.mark_partially_checked_node(file_b, "speculative new-file");
                state.rollback_speculation(checkpoint);
                assert_eq!(state.partially_checked_ranges.len(), 1);
                assert_eq!(state.partially_checked_ranges[&0].len(), 1);
                assert_eq!(state.partial_check_records.len(), records_before);
            },
        );
    }

    /// The extended assert net (B35): raw Signature caches follow the
    /// links write discipline.
    #[test]
    #[should_panic(expected = "links writes are forbidden during speculation")]
    fn erased_signature_cache_write_asserts_under_speculation() {
        with_program_state(
            &[("a.ts", "declare function f<T>(x: T): T;\n")],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                    .expect("f resolves");
                let ty = state.get_type_of_symbol(symbol).expect("f types");
                let signature = state
                    .get_signatures_of_type(ty, SignatureKind::Call)
                    .expect("f has call signatures")[0];
                state.speculation_depth = 1;
                let _ = state.get_erased_signature(signature);
            },
        );
    }

    /// The canonical twin (M6 7.5): getCanonicalSignature's cache
    /// write sits in the same net. The identity instantiation is
    /// WARMED at depth 0 first (7.5d review) — cold, the
    /// instantiations-map assert inside getSignatureInstantiation
    /// fires before the canonical write and this test would pin the
    /// wrong assert.
    #[test]
    #[should_panic(expected = "links writes are forbidden during speculation")]
    fn canonical_signature_cache_write_asserts_under_speculation() {
        with_program_state(
            &[("a.ts", "declare function f<T>(x: T): T;\n")],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                    .expect("f resolves");
                let ty = state.get_type_of_symbol(symbol).expect("f types");
                let signature = state
                    .get_signatures_of_type(ty, SignatureKind::Call)
                    .expect("f has call signatures")[0];
                let own_parameter = state
                    .signature_of(signature)
                    .type_parameters
                    .clone()
                    .expect("f is generic")[0];
                state
                    .get_signature_instantiation(
                        signature,
                        Some(&[own_parameter]),
                        /*is_javascript*/ false,
                        /*inferred_type_parameters*/ None,
                    )
                    .expect("identity instantiation warms at depth 0");
                state.speculation_depth = 1;
                let _ = state.get_canonical_signature(signature);
            },
        );
    }
}
