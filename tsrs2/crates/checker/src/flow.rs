//! M5 6.1: control-flow narrowing — prelude + FlowType + the
//! getTypeAtFlowNode walk skeleton (m5-flow-steps.md stage 6.1).
//!
//! The bind-time graph exists since M2 stage 3.5 (binder flow.rs);
//! this module is the check-time half: `getFlowTypeOfReference`
//! resolves a reference's type by walking the graph backward from the
//! reference's flow node (checker-key-functions.md §4).
//!
//! tsc SHAPE FACT (70394-70501): `getTypeAtFlowNode` and the arm
//! workers are CLOSURES inside getFlowTypeOfReference — `flowDepth`,
//! the cache `key`, and the reference/declared/initial/flowContainer
//! bindings are per-query locals, NOT checker globals. The port models
//! the query locals as [`FlowQuery`] threaded through the walk;
//! `sharedFlowNodes`/`sharedFlowTypes` ARE checker globals trimmed via
//! `sharedFlowStart` (here: one Vec truncated on query exit, ALSO on
//! Unsupported unwind — the unwind invariant).
//!
//! FlowIds are per-file (each file binds its own FlowArena); a walk
//! never leaves the reference's file — the Start outer-container
//! resume rule stays in-file by construction — so the query pins the
//! owning file index once.
//!
//! Stage state (6.4 COMPLETE): the assignment and array-mutation
//! arms (6.2), the branch/loop JOINs with the loop-label fixpoint
//! (6.3), the condition/switch-clause arms with the full narrow.rs
//! dispatch, and the call arm with effects signatures (6.4) are all
//! LIVE, and both caller initialType ladders are flipped
//! (checkIdentifier + the access.rs assume-uninitialized arm). The
//! query flag (`FlowQuery::traversed_inert_arm`) remains as the
//! narrow M6-deferral channel (see narrow.rs) until 6.6 retires the
//! failure-face gates; reachability is the 6.6 true-stub.
//!
//! THE 6.2 SEAM (retires with the 6.4 narrowers): a query that
//! crossed a still-inert arm reverts to the 6.1 answer (declared
//! type, auto-converted) at query exit — the inert arms cannot
//! reproduce tsc's narrowing, and letting their over-wide
//! pass-through answer out would misreport 2454/2565 (an initial-type
//! undefined tsc narrows away) and misfire the 7034 auto-identity
//! test. Queries whose walk meets only live arms (start/assignment/
//! mutation/join paths) get the full semantics; the ladder sites
//! partial-mark the flagged-and-suppressed diagnostic positions. The
//! loop-label fixpoint additionally refuses to CACHE a result whose
//! query is flagged: the flowLoopCaches memo outlives the query, and
//! a later same-key query hitting the memo would skip the walk — and
//! with it the flag — leaking the over-wide answer past the seam.

use tsrs2_binder::flow::{FlowId, FlowPayload};
use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, FlowFlags, NodeCheckFlags, NodeFlags, SymbolFlags, TypeData, TypeFlags, TypeId,
};

use crate::state::{CheckResult2, CheckerState, Unsupported};

/// tsc FlowType = Type | IncompleteType (checker-key §4.1). The
/// `Incomplete` wrapper means "computed while a loop back-edge was
/// still being resolved — a lower bound, not final"; it is what makes
/// loop narrowing terminate correctly (consumed by the 6.3 fixpoint).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlowType {
    Type(TypeId),
    // Produced by the 6.3 loop-label fixpoint (the in-progress
    // back-edge answer and the incomplete-first-antecedent result);
    // the Compound/array-mutation arms and the branch join re-wrap
    // their antecedents' completeness.
    Incomplete(TypeId),
}

impl FlowType {
    /// tsc-port: getTypeFromFlowType @6.0.3
    /// tsc-hash: b102170afea35b25b1b0f2ee9fa04a3359d2c31b606cd547af93348e44092dca
    /// tsc-span: _tsc.js:70067-70069
    pub(crate) fn get_type(self) -> TypeId {
        match self {
            FlowType::Type(ty) | FlowType::Incomplete(ty) => ty,
        }
    }

    /// tsc-port: isIncomplete @6.0.3
    /// tsc-hash: c858fa411b18f0051d3bf4547d2599fcec6882bc07fc14e3cd8f63c87577f3a5
    /// tsc-span: _tsc.js:70064-70066
    pub(crate) fn is_incomplete(self) -> bool {
        matches!(self, FlowType::Incomplete(_))
    }
}

/// The per-query locals of tsc getFlowTypeOfReference (70394): the
/// reference bindings plus `flowDepth` (reset per query — NOT checker
/// state), this query's window into the shared-flow cache, and the
/// loop-label cache `key`/`isKeySet` memo pair (70395/70413).
pub(crate) struct FlowQuery {
    pub(crate) reference: NodeId,
    pub(crate) declared_type: TypeId,
    pub(crate) initial_type: TypeId,
    pub(crate) flow_container: Option<NodeId>,
    /// Owning file of the reference's flow graph (FlowIds are
    /// per-file; the walk never leaves it).
    pub(crate) file: usize,
    pub(crate) flow_depth: u32,
    pub(crate) shared_flow_start: usize,
    /// tsc `key`/`isKeySet` (70395-70396): the lazily computed flow
    /// cache key — outer None = not computed yet (isKeySet false),
    /// Some(None) = computed, reference has no key.
    pub(crate) key: Option<Option<String>>,
    /// tsrs-native 6.2 SEAM (retires with the 6.4 narrowers): set
    /// when the walk crosses a still-inert arm (condition/switch) —
    /// the query exit forces such answers back to the 6.1 value
    /// (declared, auto-converted) and mirrors the flag into
    /// `CheckerState::flow_last_query_inert` for the initialType
    /// ladder sites; the loop-label fixpoint reads it to keep flagged
    /// results out of the cross-query flowLoopCaches memo.
    pub(crate) traversed_inert_arm: bool,
    /// tsc getSyntheticElementAccess (55897): a destructuring query's
    /// reference is the parse-node-factory access chain
    /// `base["p0"]["p1"]…` with the base access's flowNode. The arena
    /// is immutable to the checker, so the synthetic chain is DATA on
    /// the query — `reference` holds the real BASE node and this
    /// holds the accessed-name chain (outermost last, already
    /// escaped). Every reference-shaped probe of the walk consults it
    /// (matching, cache keys, the Start resume rule, the postlude
    /// probes); None = a plain node reference.
    pub(crate) synthetic_props: Option<Vec<String>>,
}

/// One in-progress loop-label fixpoint frame: tsc's parallel
/// flowLoopNodes[i]/flowLoopKeys[i]/flowLoopTypes[i] entry at index
/// i < flowLoopCount (the Vec length here). Published before a
/// back-edge walk so a self-referencing inner query returns the
/// partial union tagged incomplete; popped when the walk returns.
pub(crate) struct FlowLoopEntry {
    pub(crate) file: usize,
    pub(crate) flow: FlowId,
    pub(crate) key: String,
    pub(crate) types: Vec<TypeId>,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: createFlowType @6.0.3
    /// tsc-hash: 6c62e978ce2e99049bfa9f92da21d61691a7fd53aea3085dd19d06425de9526b
    /// tsc-span: _tsc.js:70070-70072
    ///
    /// `never` is replaced by silentNeverType INSIDE incomplete
    /// wrappers — "back-edge unresolved" must stay distinguishable
    /// from a real never.
    pub(crate) fn create_flow_type(&self, ty: TypeId, incomplete: bool) -> FlowType {
        if incomplete {
            FlowType::Incomplete(if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
                self.tables.intrinsics.silent_never
            } else {
                ty
            })
        } else {
            FlowType::Type(ty)
        }
    }

    // ---- flow-graph accessors (per-file arenas) ----

    /// tsc `reference.flowNode` (the canHaveFlowNode side table).
    /// tsrs-native: binder node_flow side-table read.
    pub(crate) fn flow_node_of(&self, node: NodeId) -> Option<FlowId> {
        let file = self.binder.file_index_of_node(node);
        self.binder.file(file).node_flow.get(&node).copied()
    }

    /// tsrs-native: per-file FlowArena flags read.
    pub(crate) fn flow_flags_of(&self, file: usize, flow: FlowId) -> FlowFlags {
        self.binder.file(file).flow.flow(flow).flags
    }

    /// The single antecedent of a plain (non-label) flow node.
    /// tsrs-native: per-file FlowArena antecedent read; plain kinds
    /// store exactly one antecedent (binder flow.rs shape).
    pub(crate) fn flow_antecedent(&self, file: usize, flow: FlowId) -> FlowId {
        *self
            .binder
            .file(file)
            .flow
            .flow(flow)
            .antecedent
            .first()
            .expect(
            "plain flow kinds (Assignment/Call/Condition/SwitchClause/ArrayMutation/ReduceLabel) \
             are constructed with exactly one antecedent",
        )
    }

    /// A LABEL's antecedent list, override-aware: a ReduceLabel arm in
    /// progress (try/finally) temporarily swaps the target label's
    /// antecedents — tsc mutates `target.antecedent` in place (70473);
    /// the binder graph is immutable to the checker, so the swap lives
    /// in `reduce_label_overrides` and every label read consults it.
    /// tsrs-native: override-map view over the binder arena.
    pub(crate) fn flow_label_antecedents(&self, file: usize, flow: FlowId) -> Vec<FlowId> {
        if let Some(overridden) = self.reduce_label_overrides.get(&(file, flow)) {
            return overridden.clone();
        }
        self.binder.file(file).flow.flow(flow).antecedent.clone()
    }

    fn flow_payload_node(&self, file: usize, flow: FlowId) -> Option<NodeId> {
        match self.binder.file(file).flow.flow(flow).payload {
            FlowPayload::Node(node) => Some(node),
            _ => None,
        }
    }

    // ---- the entry ----

    /// tsc-port: getFlowTypeOfReference @6.0.3
    /// tsc-hash: 2495e2c0431a9096a4037d567adf9bbe636410c94f947585e84906f969aae63e
    /// tsc-span: _tsc.js:70394-70412
    ///
    /// The prologue + postlude of the query (the closures port as the
    /// FlowQuery walk family below). The flowNode parameter defaults
    /// to the reference's own flow node; callers with an explicit one
    /// (getNarrowedTypeOfSymbol's `location.flowNode`) use
    /// `get_flow_type_of_reference_with_flow`.
    pub(crate) fn get_flow_type_of_reference(
        &mut self,
        reference: NodeId,
        declared_type: TypeId,
        initial_type: TypeId,
        flow_container: Option<NodeId>,
    ) -> CheckResult2<TypeId> {
        let flow_node = self.flow_node_of(reference);
        self.get_flow_type_of_reference_with_flow(
            reference,
            declared_type,
            initial_type,
            flow_container,
            flow_node,
        )
    }

    /// tsc-port: getFlowTypeOfReference @6.0.3 (explicit flowNode form)
    /// tsc-hash: 2495e2c0431a9096a4037d567adf9bbe636410c94f947585e84906f969aae63e
    /// tsc-span: _tsc.js:70394-70412
    pub(crate) fn get_flow_type_of_reference_with_flow(
        &mut self,
        reference: NodeId,
        declared_type: TypeId,
        initial_type: TypeId,
        flow_container: Option<NodeId>,
        flow_node: Option<FlowId>,
    ) -> CheckResult2<TypeId> {
        self.get_flow_type_of_reference_full(
            reference,
            None,
            declared_type,
            initial_type,
            flow_container,
            flow_node,
        )
    }

    /// The full-parameter body behind the plain and synthetic
    /// (destructuring) entries; `synthetic_props` carries the
    /// getSyntheticElementAccess chain when the reference is not a
    /// real node.
    /// tsrs-native: parameter split of the same tsc span.
    fn get_flow_type_of_reference_full(
        &mut self,
        reference: NodeId,
        synthetic_props: Option<Vec<String>>,
        declared_type: TypeId,
        initial_type: TypeId,
        flow_container: Option<NodeId>,
        flow_node: Option<FlowId>,
    ) -> CheckResult2<TypeId> {
        if self.flow_analysis_disabled {
            self.flow_last_query_inert = false;
            return Ok(self.tables.intrinsics.error);
        }
        let Some(flow_node) = flow_node else {
            self.flow_last_query_inert = false;
            return Ok(declared_type);
        };
        self.flow_invocation_count += 1;
        let shared_flow_start = self.shared_flow.len();
        let mut query = FlowQuery {
            reference,
            declared_type,
            initial_type,
            flow_container,
            file: self.binder.file_index_of_node(reference),
            flow_depth: 0,
            shared_flow_start,
            key: None,
            traversed_inert_arm: false,
            synthetic_props,
        };
        let walk = self.get_type_at_flow_node(&mut query, flow_node);
        // sharedFlowCount = sharedFlowStart — restored BEFORE the `?`
        // so an Unsupported unwind leaves no shared-cache residue (the
        // unwind invariant).
        self.shared_flow.truncate(shared_flow_start);
        let traversed_inert_arm = query.traversed_inert_arm;
        let result = walk.and_then(|flow_type| {
            self.flow_query_postlude(&query, flow_type.get_type(), traversed_inert_arm)
        });
        // The state mirror writes LAST: the postlude itself can run
        // NESTED flow queries (isEvolvingArrayOperationTarget types
        // the element-write index expression), and each nested query
        // overwrites the mirror — the ladder sites must read THIS
        // query's flag.
        self.flow_last_query_inert = traversed_inert_arm;
        result
    }

    /// The getFlowTypeOfReference tail (70407-70411): the 6.2
    /// inert-arm override, the evolving-array finalization, and the
    /// unreachable/NonNull declared-type reverts.
    /// tsrs-native: extracted tail of
    /// get_flow_type_of_reference_with_flow (same tsc span).
    fn flow_query_postlude(
        &mut self,
        query: &FlowQuery,
        evolved_type: TypeId,
        traversed_inert_arm: bool,
    ) -> CheckResult2<TypeId> {
        // 6.2 SEAM (retires with the 6.4 narrowers): a query that
        // crossed a still-inert condition/switch arm answers the 6.1
        // value — the declared type, auto-converted — because the
        // inert arms cannot reproduce tsc's narrowing and their
        // pass-through answer may be over-wide (an initial-type
        // undefined tsc would have narrowed away must not leak into
        // diagnostics). Queries meeting only live arms (start/
        // assignment/array-mutation/join paths) get the full
        // semantics.
        let evolved_type = if traversed_inert_arm {
            self.convert_auto_to_any(query.declared_type)?
        } else {
            evolved_type
        };
        // The getFlowTypeOfReference postlude (70408): an
        // evolving-array answer at an array-operation reference stays
        // autoArrayType; everything else finalizes.
        // Synthetic destructuring references (6.4b) skip both node
        // probes: tsc's factory node is never an array-operation
        // target (its parent is the destructuring element) and never
        // sits under a NonNullExpression.
        let result_type = if self
            .tables
            .object_flags_of(evolved_type)
            .intersects(tsrs2_types::ObjectFlags::EVOLVING_ARRAY)
            && query.synthetic_props.is_none()
            && self.is_evolving_array_operation_target(query.reference)?
        {
            self.auto_array_type()?
        } else {
            self.finalize_evolving_array_type(evolved_type)?
        };
        if result_type == self.tables.intrinsics.unreachable_never {
            return Ok(query.declared_type);
        }
        if query.synthetic_props.is_none()
            && self
                .parent_of(query.reference)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::NonNullExpression)
            && !self
                .tables
                .flags_of(result_type)
                .intersects(TypeFlags::NEVER)
        {
            // 70409: an all-nullable answer under a NonNullExpression
            // parent reverts to the declared type (live from 6.2 on —
            // the flipped initialType ladder sends real undefined
            // through straight-line queries).
            let filtered = self
                .get_type_with_facts(result_type, tsrs2_types::TypeFacts::NE_UNDEFINED_OR_NULL)?;
            if self.tables.flags_of(filtered).intersects(TypeFlags::NEVER) {
                return Ok(query.declared_type);
            }
        }
        Ok(result_type)
    }

    /// tsc-port: reportFlowControlError @6.0.3
    /// tsc-hash: 7ecd1be42943a912b0e6d7785dfa14424372e9f7196c7454a89370e2b33e9181
    /// tsc-span: _tsc.js:70234-70239
    fn report_flow_control_error(&mut self, node: NodeId) {
        let block = self.find_ancestor(Some(node), |state, n| {
            let kind = state.kind_of(n);
            let is_function_or_module_block = kind == SyntaxKind::SourceFile
                || kind == SyntaxKind::ModuleBlock
                || (kind == SyntaxKind::Block
                    && state.parent_of(n).is_some_and(|parent| {
                        node_util::is_function_like_kind(state.kind_of(parent))
                    }));
            if is_function_or_module_block {
                crate::expr::Ancestor::Yes
            } else {
                crate::expr::Ancestor::No
            }
        });
        let Some(block) = block else {
            // Every node sits under a SourceFile ancestor (a
            // SourceFile always satisfies the predicate); defensive
            // node-span fallback.
            self.error_at(
                Some(node),
                &diagnostics::The_containing_function_or_module_body_is_too_large_for_control_flow_analysis,
                &[],
            );
            return;
        };
        let source = self.binder.source_of_node(block);
        let statements_pos = match &self.data_of(block) {
            NodeData::SourceFile(data) => data.statements,
            NodeData::ModuleBlock(data) => data.statements,
            NodeData::Block(data) => data.statements,
            _ => unreachable!("isFunctionOrModuleBlock ancestors carry statements"),
        }
        .map(|statements| self.binder.node_array(statements).pos as usize)
        .unwrap_or_else(|| self.pos_of(block) as usize);
        // createFileDiagnostic over the token span at statements.pos
        // (byte offsets → UTF-16, the diagnostic surface's unit).
        let (start, end) = node_util::get_span_of_token_at_position(source, statements_pos);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start_utf16 = to_utf16(start);
        let length_utf16 = to_utf16(end).saturating_sub(start_utf16);
        let diagnostic = tsrs2_diags::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start_utf16),
            Some(length_utf16),
            tsrs2_diags::MessageChain::new(
                &diagnostics::The_containing_function_or_module_body_is_too_large_for_control_flow_analysis,
                &[],
            ),
        );
        self.diagnostics.push(diagnostic);
    }

    // ---- the backward walk ----

    /// tsc-port: getTypeAtFlowNode @6.0.3
    /// tsc-hash: d6f922eff975f535bf1d4f9a1e1957a468d81e8f1e670732ea98edb3ce4258fa
    /// tsc-span: _tsc.js:70420-70494
    ///
    /// The backward loop: flags dispatch, antecedent-walk-on-None for
    /// Assignment/Call/ArrayMutation, the per-query shared-node cache,
    /// the ReduceLabel antecedent swap (ported now — self-contained),
    /// the Start outer-container resume rule, and the depth-2000
    /// disable.
    pub(crate) fn get_type_at_flow_node(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let mut flow = flow;
        if query.flow_depth == 2000 {
            self.flow_analysis_disabled = true;
            self.report_flow_control_error(query.reference);
            return Ok(FlowType::Type(self.tables.intrinsics.error));
        }
        query.flow_depth += 1;
        let mut shared: Option<FlowId> = None;
        loop {
            let flags = self.flow_flags_of(query.file, flow);
            if flags.intersects(FlowFlags::SHARED) {
                // The shared-flow cache is scoped to THIS
                // getFlowTypeOfReference invocation (sharedFlowStart..).
                for index in query.shared_flow_start..self.shared_flow.len() {
                    if self.shared_flow[index].0 == flow {
                        query.flow_depth -= 1;
                        return Ok(self.shared_flow[index].1);
                    }
                }
                shared = Some(flow);
            }
            let ty: FlowType;
            if flags.intersects(FlowFlags::ASSIGNMENT) {
                match self.get_type_at_flow_assignment(query, flow)? {
                    Some(t) => ty = t,
                    None => {
                        flow = self.flow_antecedent(query.file, flow);
                        continue;
                    }
                }
            } else if flags.intersects(FlowFlags::CALL) {
                match self.get_type_at_flow_call(query, flow)? {
                    Some(t) => ty = t,
                    None => {
                        flow = self.flow_antecedent(query.file, flow);
                        continue;
                    }
                }
            } else if flags.intersects(FlowFlags::TRUE_CONDITION | FlowFlags::FALSE_CONDITION) {
                ty = self.get_type_at_flow_condition(query, flow)?;
            } else if flags.intersects(FlowFlags::SWITCH_CLAUSE) {
                ty = self.get_type_at_switch_clause(query, flow)?;
            } else if flags.intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL) {
                let antecedents = self.flow_label_antecedents(query.file, flow);
                if antecedents.len() == 1 {
                    flow = antecedents[0];
                    continue;
                }
                let saved_flow_depth = query.flow_depth;
                let join = if flags.intersects(FlowFlags::BRANCH_LABEL) {
                    self.get_type_at_flow_branch_label(query, flow)
                } else {
                    self.get_type_at_flow_loop_label(query, flow)
                };
                ty = match join {
                    Ok(join) => join,
                    // Reasons PREFIXED "[FLOW M5] " are the
                    // narrowable-containment GATES (failure-face
                    // checks that fire on the statement path too):
                    // they mean "tsc's answer likely differs because
                    // of narrowing" and must keep containing the
                    // enclosing statement — exactly what the pre-6.3
                    // statement-path check of the same expression
                    // did. The prefix is the discriminator: M5-owned
                    // dependency STUBS embed the tag parenthetically
                    // ("(... [FLOW M5])") and degrade below like the
                    // M6/M8 stubs — their statement-path containment
                    // stands untouched, but a join pull crossing one
                    // must not contain a statement the 6.2 label
                    // stubs let complete (the 6.3 review caught
                    // `.contains` sweeping seven stub reasons into
                    // the rethrow set).
                    Err(unsupported) if unsupported.reason.starts_with("[FLOW M5] ") => {
                        return Err(unsupported);
                    }
                    Err(_unsupported) => {
                        // [FLOW 6.3 JOIN-SEAM] Any other Unsupported
                        // inside a JOIN computation — an antecedent
                        // walk pulling a back-edge RHS, or the union's
                        // Subtype reduction relating members, through
                        // an unported M6/M8 dependency stub (e.g.
                        // lib-esnext generator machinery hitting the
                        // mapped-type stub) — degrades to the 6.2 seam
                        // instead of containing the enclosing
                        // statement: the label stubs this stage
                        // replaced never computed any of this, so
                        // statements they let complete must not
                        // regress. The flag makes the query-exit
                        // revert answer EXACTLY the 6.2 stub's value
                        // (declared, auto-converted) — every
                        // downstream diagnostic is the FP-vetted 6.2
                        // one — and keeps the flagged result out of
                        // flowLoopCaches. flow_depth rewinds to its
                        // pre-label value (the failed chain's `?`
                        // returns skip the decrements). Retires with
                        // the unported dependencies. All loop-label
                        // stack frames are popped before its Err
                        // escapes (the unwind invariant), so no state
                        // survives the catch.
                        query.flow_depth = saved_flow_depth;
                        query.traversed_inert_arm = true;
                        FlowType::Type(query.declared_type)
                    }
                };
            } else if flags.intersects(FlowFlags::ARRAY_MUTATION) {
                match self.get_type_at_flow_array_mutation(query, flow)? {
                    Some(t) => ty = t,
                    None => {
                        flow = self.flow_antecedent(query.file, flow);
                        continue;
                    }
                }
            } else if flags.intersects(FlowFlags::REDUCE_LABEL) {
                // try/finally: walk the antecedent with the target
                // label's antecedents temporarily swapped (tsc mutates
                // target.antecedent in place; the override map is the
                // immutable-arena equivalent, restored ALSO on unwind).
                let FlowPayload::ReduceLabel {
                    target,
                    ref antecedents,
                } = self.binder.file(query.file).flow.flow(flow).payload
                else {
                    unreachable!("REDUCE_LABEL flag implies ReduceLabel payload");
                };
                let swapped = antecedents.clone();
                let antecedent = self.flow_antecedent(query.file, flow);
                let saved = self
                    .reduce_label_overrides
                    .insert((query.file, target), swapped);
                let result = self.get_type_at_flow_node(query, antecedent);
                match saved {
                    Some(previous) => {
                        self.reduce_label_overrides
                            .insert((query.file, target), previous);
                    }
                    None => {
                        self.reduce_label_overrides.remove(&(query.file, target));
                    }
                }
                ty = result?;
            } else if flags.intersects(FlowFlags::START) {
                // Start outer-resume: a bare non-this reference
                // declared outside the current flow container resumes
                // in the OUTER container's flow (funcexpr/arrow
                // capture); accesses and non-arrow `this` do not.
                if let Some(container) = self.flow_payload_node(query.file, flow) {
                    // A synthetic destructuring reference (6.4b) IS an
                    // element access — accesses never resume outward.
                    let reference_kind = if query.synthetic_props.is_some() {
                        SyntaxKind::ElementAccessExpression
                    } else {
                        self.kind_of(query.reference)
                    };
                    if Some(container) != query.flow_container
                        && reference_kind != SyntaxKind::PropertyAccessExpression
                        && reference_kind != SyntaxKind::ElementAccessExpression
                        && !(reference_kind == SyntaxKind::ThisKeyword
                            && self.kind_of(container) != SyntaxKind::ArrowFunction)
                    {
                        // Start payloads are only recorded for
                        // fnexpr/arrow/object-literal-method containers
                        // (binder bind_container), whose bind_worker ran
                        // under the enclosing container's live flow and
                        // recorded node_flow; current_flow is never
                        // reset to None during subtree binding (no such
                        // assignment exists in the binder), so the
                        // container's flow node is always present.
                        flow = self
                            .flow_node_of(container)
                            .expect("Start containers record their own flowNode at bind time");
                        continue;
                    }
                }
                ty = FlowType::Type(query.initial_type);
            } else {
                // Unreachable terminus.
                ty = FlowType::Type(self.convert_auto_to_any(query.declared_type)?);
            }
            if let Some(shared_node) = shared {
                self.shared_flow.push((shared_node, ty));
            }
            query.flow_depth -= 1;
            return Ok(ty);
        }
    }

    // ---- the arms (assignment + array mutation live since 6.2,
    // branch/loop joins since 6.3, conditions/switch clauses/calls
    // since 6.4 via the narrow.rs dispatch + effects signatures) ----

    /// tsc-port: getTypeAtFlowAssignment @6.0.3
    /// tsc-hash: 06ef726ffa7b23b361ca0631413c45e9173551c164933920099df3bdafff5277
    /// tsc-span: _tsc.js:70502-70541
    ///
    /// The TS-band collapse of the containsMatchingReference
    /// expando-hoist test (70530-70535): getDeclaredExpandoInitializer
    /// reduces to the declaration's own initializer, and the only
    /// getExpandoInitializer returns surviving the caller's kind check
    /// are a DIRECT FunctionExpression/ArrowFunction initializer (the
    /// call/class/object-literal returns fail it).
    fn get_type_at_flow_assignment(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        let node = self
            .flow_payload_node(query.file, flow)
            .expect("ASSIGNMENT flow nodes carry a node payload (binder flow.rs)");
        if self.is_matching_query_reference(query, node)? {
            if !self.is_reachable_flow_node(query.file, flow)? {
                return Ok(Some(FlowType::Type(
                    self.tables.intrinsics.unreachable_never,
                )));
            }
            if self.get_assignment_target_kind(node) == crate::expr::AssignmentKind::Compound {
                let antecedent = self.flow_antecedent(query.file, flow);
                let flow_type = self.get_type_at_flow_node(query, antecedent)?;
                let base = self.get_base_type_of_literal_type(flow_type.get_type())?;
                return Ok(Some(self.create_flow_type(base, flow_type.is_incomplete())));
            }
            if query.declared_type == self.tables.intrinsics.auto
                || self.is_auto_array_type(query.declared_type)
            {
                if self.is_empty_array_assignment(node) {
                    let never = self.tables.intrinsics.never;
                    return Ok(Some(FlowType::Type(self.get_evolving_array_type(never))));
                }
                let initial_or_assigned = self.get_initial_or_assigned_type(query, node)?;
                let assigned_type = self.get_widened_literal_type(initial_or_assigned)?;
                let assignable = self.is_type_assignable_to(assigned_type, query.declared_type)?;
                return Ok(Some(FlowType::Type(if assignable {
                    assigned_type
                } else {
                    self.any_array_type()?
                })));
            }
            let t = if self.is_in_compound_like_assignment(node) {
                self.get_base_type_of_literal_type(query.declared_type)?
            } else {
                query.declared_type
            };
            if self.tables.flags_of(t).intersects(TypeFlags::UNION) {
                let assigned = self.get_initial_or_assigned_type(query, node)?;
                return Ok(Some(FlowType::Type(
                    self.get_assignment_reduced_type(t, assigned)?,
                )));
            }
            return Ok(Some(FlowType::Type(t)));
        }
        if self.contains_matching_query_reference(query, node)? {
            if !self.is_reachable_flow_node(query.file, flow)? {
                return Ok(Some(FlowType::Type(
                    self.tables.intrinsics.unreachable_never,
                )));
            }
            if self.kind_of(node) == SyntaxKind::VariableDeclaration && self.is_var_const_like(node)
            {
                let initializer = match self.data_of(node) {
                    NodeData::VariableDeclaration(data) => data.initializer,
                    _ => None,
                };
                if initializer.is_some_and(|init| {
                    matches!(
                        self.kind_of(init),
                        SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                    )
                }) {
                    let antecedent = self.flow_antecedent(query.file, flow);
                    return Ok(Some(self.get_type_at_flow_node(query, antecedent)?));
                }
            }
            return Ok(Some(FlowType::Type(query.declared_type)));
        }
        if self.kind_of(node) == SyntaxKind::VariableDeclaration {
            let grand = self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent));
            if let Some(grand) = grand {
                if self.kind_of(grand) == SyntaxKind::ForInStatement {
                    let expression = match self.data_of(grand) {
                        NodeData::ForInStatement(data) => data.expression,
                        _ => None,
                    };
                    if let Some(expression) = expression {
                        if self.is_matching_query_reference(query, expression)?
                            || self.optional_chain_contains_query_reference(expression, query)?
                        {
                            let antecedent = self.flow_antecedent(query.file, flow);
                            let walked = self.get_type_at_flow_node(query, antecedent)?;
                            let finalized = self.finalize_evolving_array_type(walked.get_type())?;
                            return Ok(Some(FlowType::Type(
                                self.get_non_nullable_type_if_needed(finalized)?,
                            )));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: getTypeAtFlowCall @6.0.3
    /// tsc-hash: f240349f4e790dee94cb460c193cd1465c8cbe3d311eaec928c71be72425d7be
    /// tsc-span: _tsc.js:70566-70587
    ///
    /// LIVE since 6.4f: an asserts-predicate signature narrows the
    /// continuation (typed asserts through narrowTypeByTypePredicate,
    /// bare `asserts x` through narrowTypeByAssertion over the
    /// argument), a never-returning signature terminates it as
    /// unreachable, and everything else walks the antecedent (None).
    fn get_type_at_flow_call(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        let node = self
            .flow_payload_node(query.file, flow)
            .expect("CALL flow nodes carry a node payload (binder flow.rs)");
        let Some(signature) = self.get_effects_signature(Some(query), node)? else {
            return Ok(None);
        };
        let predicate = self.get_type_predicate_of_signature(signature)?;
        if let Some(predicate) = predicate {
            if matches!(
                predicate.kind,
                crate::narrow::TypePredicateKind::AssertsThis
                    | crate::narrow::TypePredicateKind::AssertsIdentifier
            ) {
                let antecedent = self.flow_antecedent(query.file, flow);
                let flow_type = self.get_type_at_flow_node(query, antecedent)?;
                let ty = self.finalize_evolving_array_type(flow_type.get_type())?;
                let narrowed_type = if predicate.ty.is_some() {
                    self.narrow_type_by_type_predicate(query, ty, &predicate, node, true)?
                } else if predicate.kind == crate::narrow::TypePredicateKind::AssertsIdentifier
                    && predicate.parameter_index >= 0
                {
                    let arguments = match self.data_of(node) {
                        NodeData::CallExpression(data) => data.arguments,
                        _ => None,
                    };
                    let arguments: Vec<NodeId> = arguments
                        .map(|arguments| self.binder.node_array(arguments).nodes.clone())
                        .unwrap_or_default();
                    match usize::try_from(predicate.parameter_index)
                        .ok()
                        .and_then(|index| arguments.get(index).copied())
                    {
                        Some(argument) => self.narrow_type_by_assertion(query, ty, argument)?,
                        None => ty,
                    }
                } else {
                    ty
                };
                return Ok(Some(if narrowed_type == ty {
                    flow_type
                } else {
                    self.create_flow_type(narrowed_type, flow_type.is_incomplete())
                }));
            }
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        if self
            .tables
            .flags_of(return_type)
            .intersects(TypeFlags::NEVER)
        {
            return Ok(Some(FlowType::Type(
                self.tables.intrinsics.unreachable_never,
            )));
        }
        Ok(None)
    }

    /// tsc-port: getTypeAtFlowCondition @6.0.3
    /// tsc-hash: 328e509f8b61bcb0e868b8f29358dad7ce31deb3a0166d8f907204841274c2d7
    /// tsc-span: _tsc.js:70614-70627
    ///
    /// The arm is LIVE since 6.4a (the narrow_type dispatch is real;
    /// still-stubbed sub-narrowers flag the query themselves — see
    /// narrow.rs). A never antecedent short-circuits (dead edge), an
    /// identity narrowing returns the antecedent's FlowType unchanged
    /// (preserving Incomplete-ness), and a real narrowing re-wraps
    /// with the antecedent's completeness.
    fn get_type_at_flow_condition(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let antecedent = self.flow_antecedent(query.file, flow);
        let flow_type = self.get_type_at_flow_node(query, antecedent)?;
        let ty = flow_type.get_type();
        if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
            return Ok(flow_type);
        }
        let assume_true = self
            .flow_flags_of(query.file, flow)
            .intersects(FlowFlags::TRUE_CONDITION);
        let non_evolving_type = self.finalize_evolving_array_type(ty)?;
        let node = self
            .flow_payload_node(query.file, flow)
            .expect("Condition flows carry their expression payload (binder createFlowCondition)");
        let narrowed_type = self.narrow_type(query, non_evolving_type, node, assume_true)?;
        if narrowed_type == non_evolving_type {
            return Ok(flow_type);
        }
        Ok(self.create_flow_type(narrowed_type, flow_type.is_incomplete()))
    }

    /// tsc-port: getTypeAtSwitchClause @6.0.3
    /// tsc-hash: 564b6446e18668e072b934f29840f999d6fad7f2513be66bb5147bc7c158c26f
    /// tsc-span: _tsc.js:70628-70652
    ///
    /// LIVE since 6.4e: a matching discriminant narrows by the clause
    /// range, a matching typeof operand by the witnesses, a `switch
    /// (true)` by the clause conditions; otherwise the strict
    /// optional-chain containment strips and the discriminant-
    /// property path apply. Completeness re-wraps the antecedent's.
    fn get_type_at_switch_clause(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let FlowPayload::SwitchClause {
            switch_statement,
            clause_start,
            clause_end,
        } = self.binder.file(query.file).flow.flow(flow).payload
        else {
            unreachable!("SWITCH_CLAUSE flag implies SwitchClause payload");
        };
        let (clause_start, clause_end) = (clause_start as usize, clause_end as usize);
        let switch_expression = match self.data_of(switch_statement) {
            NodeData::SwitchStatement(data) => data.expression,
            _ => None,
        };
        let antecedent = self.flow_antecedent(query.file, flow);
        let flow_type = self.get_type_at_flow_node(query, antecedent)?;
        let mut ty = flow_type.get_type();
        let Some(switch_expression) = switch_expression else {
            // Parser-recovery switch without an expression —
            // unreproducible, flag (declared-type revert).
            query.traversed_inert_arm = true;
            return Ok(self.create_flow_type(ty, flow_type.is_incomplete()));
        };
        let expr = self.skip_parentheses(switch_expression);
        if self.is_matching_query_reference(query, expr)? {
            ty = self.narrow_type_by_switch_on_discriminant(
                ty,
                switch_statement,
                clause_start,
                clause_end,
            )?;
        } else if self.kind_of(expr) == SyntaxKind::TypeOfExpression
            && match self.data_of(expr) {
                NodeData::TypeOfExpression(data) => match data.expression {
                    Some(operand) => self.is_matching_query_reference(query, operand)?,
                    None => false,
                },
                _ => false,
            }
        {
            ty = self.narrow_type_by_switch_on_type_of(
                ty,
                switch_statement,
                clause_start,
                clause_end,
            )?;
        } else if self.kind_of(expr) == SyntaxKind::TrueKeyword {
            ty = self.narrow_type_by_switch_on_true(
                query,
                ty,
                switch_statement,
                clause_start,
                clause_end,
            )?;
        } else {
            if self
                .options
                .strict_option_value(self.options.strict_null_checks)
            {
                if self.optional_chain_contains_query_reference(expr, query)? {
                    ty = self.narrow_type_by_switch_optional_chain_containment(
                        ty,
                        switch_statement,
                        clause_start,
                        clause_end,
                        |state, t| {
                            !state.tables.flags_of(t).intersects(TypeFlags::from_bits(
                                TypeFlags::UNDEFINED.bits() | TypeFlags::NEVER.bits(),
                            ))
                        },
                    )?;
                } else if self.kind_of(expr) == SyntaxKind::TypeOfExpression {
                    let operand = match self.data_of(expr) {
                        NodeData::TypeOfExpression(data) => data.expression,
                        _ => None,
                    };
                    if let Some(operand) = operand {
                        if self.optional_chain_contains_query_reference(operand, query)? {
                            ty = self.narrow_type_by_switch_optional_chain_containment(
                                ty,
                                switch_statement,
                                clause_start,
                                clause_end,
                                |state, t| {
                                    let flags = state.tables.flags_of(t);
                                    if flags.intersects(TypeFlags::NEVER) {
                                        return false;
                                    }
                                    let is_undefined_literal = flags
                                        .intersects(TypeFlags::STRING_LITERAL)
                                        && matches!(
                                            &state.tables.type_of(t).data,
                                            TypeData::Literal {
                                                value: tsrs2_types::LiteralValue::String(value)
                                            } if value == "undefined"
                                        );
                                    !is_undefined_literal
                                },
                            )?;
                        }
                    }
                }
            }
            if let Some(access) = self.get_discriminant_property_access(query, expr, ty)? {
                ty = self.narrow_type_by_switch_on_discriminant_property(
                    ty,
                    access,
                    switch_statement,
                    clause_start,
                    clause_end,
                )?;
            }
        }
        Ok(self.create_flow_type(ty, flow_type.is_incomplete()))
    }

    /// An antecedent that is an EMPTY switch clause (clauseStart ==
    /// clauseEnd — the binder's artificial no-clause-matched edge) is
    /// deferred as the branch label's bypassFlow.
    /// tsrs-native: the inline antecedent test of
    /// getTypeAtFlowBranchLabel (70659).
    fn is_empty_switch_clause_flow(&self, file: usize, flow: FlowId) -> bool {
        self.flow_flags_of(file, flow)
            .intersects(FlowFlags::SWITCH_CLAUSE)
            && matches!(
                self.binder.file(file).flow.flow(flow).payload,
                FlowPayload::SwitchClause {
                    clause_start,
                    clause_end,
                    ..
                } if clause_start == clause_end
            )
    }

    /// The branch-label bypass consult — REAL since 6.4e (the
    /// conservative-false stub became observable the moment the
    /// switch-clause arm went live; narrow.rs carries the pulled-
    /// forward isExhaustiveSwitchStatement/compute pair, and its
    /// remaining consumers — unreachable code, implicit returns —
    /// stay 6.6).
    /// tsrs-native: Option shim over the narrow.rs port.
    fn is_exhaustive_switch_statement(
        &mut self,
        switch_statement: Option<NodeId>,
    ) -> CheckResult2<bool> {
        match switch_statement {
            Some(switch_statement) => self.is_exhaustive_switch_statement_real(switch_statement),
            None => Ok(false),
        }
    }

    /// tsc-port: getTypeAtFlowBranchLabel @6.0.3
    /// tsc-hash: 42a133e73000df9b5c537b6d2ea51a69a62e9fc2599264e382f1130505a81263
    /// tsc-span: _tsc.js:70653-70693
    ///
    /// The JOIN: union of the antecedent walks, with the
    /// empty-switch-clause bypass (deferred to after the loop, skipped
    /// when never/duplicate/exhaustive) and the declared-type
    /// short-circuit (an antecedent equal to an unnarrowed declared
    /// type makes the whole join that type).
    fn get_type_at_flow_branch_label(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let mut antecedent_types: Vec<TypeId> = Vec::new();
        let mut subtype_reduction = false;
        let mut seen_incomplete = false;
        let mut bypass_flow: Option<FlowId> = None;
        for antecedent in self.flow_label_antecedents(query.file, flow) {
            if bypass_flow.is_none() && self.is_empty_switch_clause_flow(query.file, antecedent) {
                bypass_flow = Some(antecedent);
                continue;
            }
            let flow_type = self.get_type_at_flow_node(query, antecedent)?;
            let ty = flow_type.get_type();
            if ty == query.declared_type && query.declared_type == query.initial_type {
                return Ok(FlowType::Type(ty));
            }
            if !antecedent_types.contains(&ty) {
                antecedent_types.push(ty);
            }
            if !self.is_type_subset_of(ty, query.initial_type)? {
                subtype_reduction = true;
            }
            if flow_type.is_incomplete() {
                seen_incomplete = true;
            }
        }
        if let Some(bypass) = bypass_flow {
            // The default/no-match path of a non-exhaustive switch
            // rejoins here.
            let flow_type = self.get_type_at_flow_node(query, bypass)?;
            let ty = flow_type.get_type();
            let switch_statement = match self.binder.file(query.file).flow.flow(bypass).payload {
                FlowPayload::SwitchClause {
                    switch_statement, ..
                } => Some(switch_statement),
                _ => None,
            };
            if !self.tables.flags_of(ty).intersects(TypeFlags::NEVER)
                && !antecedent_types.contains(&ty)
                && !self.is_exhaustive_switch_statement(switch_statement)?
            {
                if ty == query.declared_type && query.declared_type == query.initial_type {
                    return Ok(FlowType::Type(ty));
                }
                antecedent_types.push(ty);
                if !self.is_type_subset_of(ty, query.initial_type)? {
                    subtype_reduction = true;
                }
                if flow_type.is_incomplete() {
                    seen_incomplete = true;
                }
            }
        }
        let union = self.get_union_or_evolving_array_type(
            query,
            &antecedent_types,
            if subtype_reduction {
                tsrs2_types::UnionReduction::Subtype
            } else {
                tsrs2_types::UnionReduction::Literal
            },
        )?;
        Ok(self.create_flow_type(union, seen_incomplete))
    }

    /// tsc-port: getTypeAtFlowLoopLabel @6.0.3
    /// tsc-hash: e2be2f0eb5db01a25e38653a320096d0e34cef40629baedbbc0c1c53445bc027
    /// tsc-span: _tsc.js:70694-70755
    ///
    /// THE fixpoint (checker-key §4.4). The first antecedent (loop
    /// entry) resolves normally; back-edge antecedents resolve with
    /// the current partial union published on the flow-loop stack, so
    /// a self-reference returns the accumulated-so-far union tagged
    /// INCOMPLETE. Terminates because each pass only adds members.
    /// Its two non-negotiables:
    /// - the flowTypeCache swap during back-edge resolution (partial
    ///   results must not enter getTypeOfExpression's cache);
    /// - never caching while the first antecedent is incomplete.
    ///
    /// 6.2-SEAM EXTENSION (tsrs-native, retires with 6.4): a result
    /// computed through a still-inert arm (`query.traversed_inert_arm`)
    /// is answered but NOT cached — flowLoopCaches outlives the query,
    /// and a later same-key query hitting the memo would skip the
    /// walk (and the flag), leaking the over-wide answer past the
    /// query-exit revert. The query-global flag over-approximates the
    /// fixpoint's own subtree, so the guard never caches a dirty
    /// result; once 6.4 retires the flag it is constant-false and the
    /// tsc shape is exact.
    fn get_type_at_flow_loop_label(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        // getFlowNodeId(flow) → flowLoopCaches[id]: FlowIds are
        // per-file, so (file, FlowId) is the cache identity.
        let Some(key) = self.get_or_set_cache_key(query)? else {
            return Ok(FlowType::Type(query.declared_type));
        };
        if let Some(cache) = self.flow_loop_caches.get(&(query.file, flow)) {
            if let Some(&cached) = cache.get(&key) {
                return Ok(FlowType::Type(cached));
            }
        }
        // An in-progress back-edge resolution of this same label+key:
        // answer the partial union as INCOMPLETE.
        for index in self.flow_loop_start as usize..self.flow_loop_stack.len() {
            let entry = &self.flow_loop_stack[index];
            if entry.file == query.file
                && entry.flow == flow
                && entry.key == key
                && !entry.types.is_empty()
            {
                let types = entry.types.clone();
                let union = self.get_union_or_evolving_array_type(
                    query,
                    &types,
                    tsrs2_types::UnionReduction::Literal,
                )?;
                return Ok(self.create_flow_type(union, true));
            }
        }
        let mut antecedent_types: Vec<TypeId> = Vec::new();
        let mut subtype_reduction = false;
        let mut first_antecedent_type: Option<FlowType> = None;
        for antecedent in self.flow_label_antecedents(query.file, flow) {
            let flow_type = if first_antecedent_type.is_none() {
                let first = self.get_type_at_flow_node(query, antecedent)?;
                first_antecedent_type = Some(first);
                first
            } else {
                // Publish the partial union for the back-edge walk and
                // clear the flow-type cache: types computed mid-loop
                // are lower bounds and must not be cached (the first
                // non-negotiable). Pop + restore BEFORE `?` so an
                // Unsupported unwind leaves no frame behind (the
                // unwind invariant) — the walk dispatch's join-seam
                // catch relies on it.
                self.flow_loop_stack.push(FlowLoopEntry {
                    file: query.file,
                    flow,
                    key: key.clone(),
                    types: antecedent_types.clone(),
                });
                let save_flow_type_cache = self.flow_type_cache.take();
                let walked = self.get_type_at_flow_node(query, antecedent);
                self.flow_type_cache = save_flow_type_cache;
                self.flow_loop_stack.pop();
                let flow_type = walked?;
                // The fixpoint may have been finalized during the
                // recursive walk (a nested query completed it).
                if let Some(cache) = self.flow_loop_caches.get(&(query.file, flow)) {
                    if let Some(&cached) = cache.get(&key) {
                        return Ok(FlowType::Type(cached));
                    }
                }
                flow_type
            };
            let ty = flow_type.get_type();
            if !antecedent_types.contains(&ty) {
                antecedent_types.push(ty);
            }
            if !self.is_type_subset_of(ty, query.initial_type)? {
                subtype_reduction = true;
            }
            // Reached the widest possible answer; no further pass can
            // widen it.
            if ty == query.declared_type {
                break;
            }
        }
        let result = self.get_union_or_evolving_array_type(
            query,
            &antecedent_types,
            if subtype_reduction {
                tsrs2_types::UnionReduction::Subtype
            } else {
                tsrs2_types::UnionReduction::Literal
            },
        )?;
        if first_antecedent_type.is_some_and(FlowType::is_incomplete) {
            // The second non-negotiable: never cache while incomplete.
            return Ok(self.create_flow_type(result, true));
        }
        if !query.traversed_inert_arm {
            self.flow_loop_caches
                .entry((query.file, flow))
                .or_default()
                .insert(key, result);
        }
        Ok(FlowType::Type(result))
    }

    /// tsc-port: getOrSetCacheKey @6.0.3
    /// tsc-hash: e224dd54f074c1f3304e88be454bdbbd8ae9105ce337bfb6b83b6d39ce4beef7
    /// tsc-span: _tsc.js:70413-70419
    fn get_or_set_cache_key(&mut self, query: &mut FlowQuery) -> CheckResult2<Option<String>> {
        if let Some(key) = &query.key {
            return Ok(key.clone());
        }
        let base_key = self.get_flow_cache_key(
            query.reference,
            query.declared_type,
            query.initial_type,
            query.flow_container,
        )?;
        // A synthetic destructuring chain keys as tsc's synthetic
        // node does: the base access's key with each accessed name
        // appended (getFlowCacheKey's access arm per level).
        let key = match (&query.synthetic_props, base_key) {
            (Some(props), Some(base_key)) => Some(format!("{base_key}.{}", props.join("."))),
            (Some(_), None) => None,
            (None, base_key) => base_key,
        };
        query.key = Some(key.clone());
        Ok(key)
    }

    /// tsc-port: getFlowCacheKey @6.0.3
    /// tsc-hash: 680d4522c23b10729b148621ff3f5b43e22e5a17351b666d2ef862ede1d455ed
    /// tsc-span: _tsc.js:69407-69447
    ///
    /// The key strings mirror tsc's exactly, with tsrs2's stable ids
    /// standing in for tsc's lazily assigned node/type/symbol ids
    /// (the key only needs identity, not any particular numbering).
    fn get_flow_cache_key(
        &mut self,
        node: NodeId,
        declared_type: TypeId,
        initial_type: TypeId,
        flow_container: Option<NodeId>,
    ) -> CheckResult2<Option<String>> {
        let container_id = flow_container.map_or_else(|| "-1".to_owned(), |c| c.0.to_string());
        match self.kind_of(node) {
            SyntaxKind::Identifier if !self.is_this_in_type_query(node) => {
                Ok(self.get_resolved_symbol(node)?.map(|symbol| {
                    format!(
                        "{container_id}|{}|{}|{}",
                        declared_type.0, initial_type.0, symbol.0
                    )
                }))
            }
            // The this-in-type-query Identifier falls through to the
            // ThisKeyword arm, exactly as tsc's switch does.
            SyntaxKind::Identifier | SyntaxKind::ThisKeyword => Ok(Some(format!(
                "0|{container_id}|{}|{}",
                declared_type.0, initial_type.0
            ))),
            SyntaxKind::NonNullExpression => {
                let NodeData::NonNullExpression(data) = self.data_of(node) else {
                    return Ok(None);
                };
                match data.expression {
                    Some(expression) => self.get_flow_cache_key(
                        expression,
                        declared_type,
                        initial_type,
                        flow_container,
                    ),
                    None => Ok(None),
                }
            }
            SyntaxKind::ParenthesizedExpression => {
                let NodeData::ParenthesizedExpression(data) = self.data_of(node) else {
                    return Ok(None);
                };
                match data.expression {
                    Some(expression) => self.get_flow_cache_key(
                        expression,
                        declared_type,
                        initial_type,
                        flow_container,
                    ),
                    None => Ok(None),
                }
            }
            SyntaxKind::QualifiedName => {
                let NodeData::QualifiedName(data) = self.data_of(node) else {
                    return Ok(None);
                };
                let (Some(left), Some(right)) = (data.left, data.right) else {
                    return Ok(None);
                };
                let Some(left_key) =
                    self.get_flow_cache_key(left, declared_type, initial_type, flow_container)?
                else {
                    return Ok(None);
                };
                let Some(right_text) = self.escaped_text_of(Some(right)) else {
                    return Ok(None);
                };
                Ok(Some(format!("{left_key}.{right_text}")))
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                let expression = match self.data_of(node) {
                    NodeData::PropertyAccessExpression(data) => data.expression,
                    NodeData::ElementAccessExpression(data) => data.expression,
                    _ => None,
                };
                let Some(expression) = expression else {
                    return Ok(None);
                };
                if let Some(prop_name) = self.get_accessed_property_name(node)? {
                    let Some(key) = self.get_flow_cache_key(
                        expression,
                        declared_type,
                        initial_type,
                        flow_container,
                    )?
                    else {
                        return Ok(None);
                    };
                    return Ok(Some(format!("{key}.{prop_name}")));
                }
                if self.kind_of(node) == SyntaxKind::ElementAccessExpression {
                    let argument = match self.data_of(node) {
                        NodeData::ElementAccessExpression(data) => data.argument_expression,
                        _ => None,
                    };
                    if let Some(argument) = argument {
                        if self.kind_of(argument) == SyntaxKind::Identifier {
                            if let Some(symbol) = self.get_resolved_symbol(argument)? {
                                if self.is_constant_variable(symbol)
                                    || (self.is_parameter_or_mutable_local_variable(symbol)
                                        && !self.is_symbol_assigned(symbol)?)
                                {
                                    let Some(key) = self.get_flow_cache_key(
                                        expression,
                                        declared_type,
                                        initial_type,
                                        flow_container,
                                    )?
                                    else {
                                        return Ok(None);
                                    };
                                    return Ok(Some(format!("{key}.@{}", symbol.0)));
                                }
                            }
                        }
                    }
                }
                Ok(None)
            }
            SyntaxKind::ObjectBindingPattern
            | SyntaxKind::ArrayBindingPattern
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration => Ok(Some(format!("{}#{}", node.0, declared_type.0))),
            _ => Ok(None),
        }
    }

    /// tsc-port: getUnionOrEvolvingArrayType @6.0.3
    /// tsc-hash: 1bcdcc9a640a41919d106cc96d516819da508516e1aeb44c9d4117a555b4a9df
    /// tsc-span: _tsc.js:70756-70765
    ///
    /// The JOIN's union: an all-evolving-array list joins element
    /// types into one evolving array; otherwise the members finalize,
    /// union, recombine `{}|null|undefined` back to unknown, and a
    /// member-identical union answers the IDENTICAL declaredType
    /// object (identity preservation feeds the branch short-circuit).
    fn get_union_or_evolving_array_type(
        &mut self,
        query: &FlowQuery,
        types: &[TypeId],
        subtype_reduction: tsrs2_types::UnionReduction,
    ) -> CheckResult2<TypeId> {
        if self.is_evolving_array_type_list(types) {
            let element_types: Vec<TypeId> = types
                .iter()
                .map(|&ty| self.get_element_type_of_evolving_array_type(ty))
                .collect();
            let union =
                self.get_union_type_ex(&element_types, tsrs2_types::UnionReduction::Literal)?;
            return Ok(self.get_evolving_array_type(union));
        }
        let mut finalized = Vec::with_capacity(types.len());
        for &ty in types {
            finalized.push(self.finalize_evolving_array_type(ty)?);
        }
        let union = self.get_union_type_ex(&finalized, subtype_reduction)?;
        let result = self.recombine_unknown_type(union);
        if result != query.declared_type
            && self.tables.flags_of(result).intersects(TypeFlags::UNION)
            && self
                .tables
                .flags_of(query.declared_type)
                .intersects(TypeFlags::UNION)
        {
            let TypeData::Union {
                types: result_types,
                ..
            } = &self.tables.type_of(result).data
            else {
                unreachable!("union flag implies union data");
            };
            let TypeData::Union {
                types: declared_types,
                ..
            } = &self.tables.type_of(query.declared_type).data
            else {
                unreachable!("union flag implies union data");
            };
            if result_types == declared_types {
                return Ok(query.declared_type);
            }
        }
        Ok(result)
    }

    /// tsc-port: getTypeAtFlowArrayMutation @6.0.3
    /// tsc-hash: deb98a2cbf7993883c992363c63d80d5c290ac8fbaddae91e5ddfb9c684730b3
    /// tsc-span: _tsc.js:70588-70613
    fn get_type_at_flow_array_mutation(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        if query.declared_type != self.tables.intrinsics.auto
            && !self.is_auto_array_type(query.declared_type)
        {
            return Ok(None);
        }
        let node = self
            .flow_payload_node(query.file, flow)
            .expect("ARRAY_MUTATION flow nodes carry a node payload (binder flow.rs)");
        // CallExpression: `a.push(...)`/`a.unshift(...)` — the
        // receiver is node.expression.expression; otherwise the
        // element write `a[i] = ...` — node.left.expression.
        let (expr, call_arguments, element_write) = match self.data_of(node) {
            NodeData::CallExpression(data) => {
                let receiver = data
                    .expression
                    .and_then(|callee| self.access_expression_of(callee));
                (receiver, data.arguments, None)
            }
            NodeData::BinaryExpression(data) => {
                let left = data.left;
                let receiver = left.and_then(|left| self.access_expression_of(left));
                let argument = left.and_then(|left| self.element_access_argument_of(left));
                (receiver, None, Some((argument, data.right)))
            }
            _ => (None, None, None),
        };
        let Some(expr) = expr else {
            return Ok(None);
        };
        let candidate = self.get_reference_candidate(expr);
        if !self.is_matching_query_reference(query, candidate)? {
            return Ok(None);
        }
        let antecedent = self.flow_antecedent(query.file, flow);
        let flow_type = self.get_type_at_flow_node(query, antecedent)?;
        let ty = flow_type.get_type();
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(tsrs2_types::ObjectFlags::EVOLVING_ARRAY)
        {
            return Ok(Some(flow_type));
        }
        let mut evolved = ty;
        if let Some(arguments) = call_arguments {
            let arguments: Vec<NodeId> = self.binder.node_array(arguments).nodes.clone();
            for argument in arguments {
                evolved = self.add_evolving_array_element_type(evolved, argument)?;
            }
        } else if let Some((argument, right)) = element_write {
            let (Some(argument), Some(right)) = (argument, right) else {
                return Ok(Some(flow_type));
            };
            let index_type = self.get_context_free_type_of_expression(argument)?;
            if self.is_type_assignable_to_kind(
                index_type,
                TypeFlags::NUMBER_LIKE,
                /*strict*/ false,
            )? {
                evolved = self.add_evolving_array_element_type(evolved, right)?;
            }
        }
        Ok(Some(if evolved == ty {
            flow_type
        } else {
            self.create_flow_type(evolved, flow_type.is_incomplete())
        }))
    }

    // ---- reachability (6.6) ----

    /// tsc-port: isReachableFlowNode @6.0.3
    /// tsc-hash: 53516d2070f9cc88e8f8628e42da319584859572a57db01f35d0307a0a90a354
    /// tsc-span: _tsc.js:70240-70249
    ///
    /// The worker + the single-entry memo update (tsc writes
    /// lastFlowNode AFTER the walk — the worker's own top-of-loop
    /// consult sees the PREVIOUS query's entry). Fallible tsrs-side:
    /// the Call arm's effects consult can defer (M6 body-inference
    /// candidates — narrow.rs get_effects_signature's None-query
    /// contract) and its signature resolution can unwind; Err leaves
    /// both memos unwritten, so no undecided verdict outlives the
    /// failed walk.
    pub(crate) fn is_reachable_flow_node(
        &mut self,
        file: usize,
        flow: FlowId,
    ) -> CheckResult2<bool> {
        let result = self.is_reachable_flow_node_worker(file, flow, false)?;
        self.last_flow_node = Some((file, flow));
        self.last_flow_node_reachable = result;
        Ok(result)
    }

    /// tsc-port: isFalseExpression @6.0.3
    /// tsc-hash: 51793c17d5ebe4d6c317e3702da5258a5b39749dacdf78398b78acc35f4c019f
    /// tsc-span: _tsc.js:70250-70257
    ///
    /// tsc passes skipParentheses excludeJSDocTypeAssertions=true; the
    /// evaluator's paren-only skip is exact for TS sources (a JSDoc
    /// type assertion is a JS-file parse shape, and the checked band
    /// is TS) — same collapse as the evaluator's own header notes.
    fn is_false_expression(&self, expr: NodeId) -> bool {
        let node = self.skip_parentheses(expr);
        match self.kind_of(node) {
            SyntaxKind::FalseKeyword => true,
            SyntaxKind::BinaryExpression => {
                let NodeData::BinaryExpression(data) = self.data_of(node) else {
                    return false;
                };
                let (Some(left), Some(operator), Some(right)) =
                    (data.left, data.operator_token, data.right)
                else {
                    return false;
                };
                match self.kind_of(operator) {
                    SyntaxKind::AmpersandAmpersandToken => {
                        self.is_false_expression(left) || self.is_false_expression(right)
                    }
                    SyntaxKind::BarBarToken => {
                        self.is_false_expression(left) && self.is_false_expression(right)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// tsc-port: isReachableFlowNodeWorker @6.0.3
    /// tsc-hash: ecda265059756d8e54e8674c4147c48d359376f41a068ab76ceb5bafd6c70775
    /// tsc-span: _tsc.js:70258-70327
    ///
    /// Straight-line runs iterate; BranchLabel antecedents and the
    /// Shared-memo compute recurse like tsc (depth is branch-nesting,
    /// not statement count — the 6.3 join walk set the precedent).
    /// LOOP_LABEL takes antecedents[0], the entry edge, so back-edges
    /// never cycle the walk (and the Shared miss-compute cannot
    /// re-enter itself). Label reads are override-aware
    /// (flow_label_antecedents); the ReduceLabel arm invalidates the
    /// single-entry memo like tsc (70312) and restores the swap on Ok
    /// AND on unwind. The Shared arm's noCacheCheck reset falls
    /// through into the SAME iteration's arm dispatch (tsc 70272 —
    /// only the NEXT shared node consults the memo again).
    fn is_reachable_flow_node_worker(
        &mut self,
        file: usize,
        mut flow: FlowId,
        mut no_cache_check: bool,
    ) -> CheckResult2<bool> {
        loop {
            if self.last_flow_node == Some((file, flow)) {
                return Ok(self.last_flow_node_reachable);
            }
            let flags = self.flow_flags_of(file, flow);
            if flags.intersects(FlowFlags::SHARED) {
                if !no_cache_check {
                    if let Some(&reachable) = self.flow_node_reachable.get(&(file, flow)) {
                        return Ok(reachable);
                    }
                    let reachable = self.is_reachable_flow_node_worker(file, flow, true)?;
                    self.flow_node_reachable.insert((file, flow), reachable);
                    return Ok(reachable);
                }
                no_cache_check = false;
            }
            if flags.intersects(
                FlowFlags::ASSIGNMENT | FlowFlags::CONDITION | FlowFlags::ARRAY_MUTATION,
            ) {
                flow = self.flow_antecedent(file, flow);
            } else if flags.intersects(FlowFlags::CALL) {
                let node = self
                    .flow_payload_node(file, flow)
                    .expect("CALL flow nodes carry a node payload (binder flow.rs)");
                if let Some(signature) = self.get_effects_signature(None, node)? {
                    if let Some(predicate) = self.get_type_predicate_of_signature(signature)? {
                        if predicate.kind == crate::narrow::TypePredicateKind::AssertsIdentifier
                            && predicate.ty.is_none()
                        {
                            let arguments = match self.data_of(node) {
                                NodeData::CallExpression(data) => data.arguments,
                                _ => None,
                            };
                            let argument = usize::try_from(predicate.parameter_index)
                                .ok()
                                .and_then(|index| {
                                    arguments.and_then(|arguments| {
                                        self.binder.node_array(arguments).nodes.get(index).copied()
                                    })
                                });
                            if argument.is_some_and(|argument| self.is_false_expression(argument)) {
                                return Ok(false);
                            }
                        }
                    }
                    let return_type = self.get_return_type_of_signature(signature)?;
                    if self
                        .tables
                        .flags_of(return_type)
                        .intersects(TypeFlags::NEVER)
                    {
                        return Ok(false);
                    }
                }
                flow = self.flow_antecedent(file, flow);
            } else if flags.intersects(FlowFlags::BRANCH_LABEL) {
                for antecedent in self.flow_label_antecedents(file, flow) {
                    if self.is_reachable_flow_node_worker(file, antecedent, false)? {
                        return Ok(true);
                    }
                }
                return Ok(false);
            } else if flags.intersects(FlowFlags::LOOP_LABEL) {
                let antecedents = self.flow_label_antecedents(file, flow);
                match antecedents.first() {
                    Some(&entry_edge) => flow = entry_edge,
                    None => return Ok(false),
                }
            } else if flags.intersects(FlowFlags::SWITCH_CLAUSE) {
                let FlowPayload::SwitchClause {
                    switch_statement,
                    clause_start,
                    clause_end,
                } = self.binder.file(file).flow.flow(flow).payload
                else {
                    unreachable!("SWITCH_CLAUSE flag implies SwitchClause payload");
                };
                if clause_start == clause_end
                    && self.is_exhaustive_switch_statement(Some(switch_statement))?
                {
                    return Ok(false);
                }
                flow = self.flow_antecedent(file, flow);
            } else if flags.intersects(FlowFlags::REDUCE_LABEL) {
                self.last_flow_node = None;
                let FlowPayload::ReduceLabel {
                    target,
                    ref antecedents,
                } = self.binder.file(file).flow.flow(flow).payload
                else {
                    unreachable!("REDUCE_LABEL flag implies ReduceLabel payload");
                };
                let swapped = antecedents.clone();
                let antecedent = self.flow_antecedent(file, flow);
                let saved = self.reduce_label_overrides.insert((file, target), swapped);
                let result = self.is_reachable_flow_node_worker(file, antecedent, false);
                match saved {
                    Some(previous) => {
                        self.reduce_label_overrides.insert((file, target), previous);
                    }
                    None => {
                        self.reduce_label_overrides.remove(&(file, target));
                    }
                }
                return result;
            } else {
                return Ok(!flags.intersects(FlowFlags::UNREACHABLE));
            }
        }
    }

    // ---- initial/assigned types (the assignment arm's inputs) ----

    /// tsc-port: getInitialOrAssignedType @6.0.3
    /// tsc-hash: e3070980824824baba9d0cf8ac38e6320def32d5a76c367122e85d4ec9d61ab8
    /// tsc-span: _tsc.js:70495-70501
    fn get_initial_or_assigned_type(
        &mut self,
        query: &mut FlowQuery,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        let ty = if matches!(
            self.kind_of(node),
            SyntaxKind::VariableDeclaration | SyntaxKind::BindingElement
        ) {
            self.get_initial_type(node)?
        } else {
            self.get_assigned_type(node)?
        };
        if query.synthetic_props.is_some() {
            // tsc probes the synthetic node's shape here
            // (isConstraintPosition on its parent = the destructuring
            // element, contextual-type probes on the synthetic
            // access): both come out false for the factory node, so
            // the substitution never applies and tsc's answer is
            // `ty`. The one probe we cannot mirror node-free is the
            // synthetic access's contextual type — flag the rare
            // generic-union-constraint candidates instead of guessing
            // (declared-type revert, FP-safe).
            if self.some_type_result(ty, |state, t| {
                state.is_generic_type_with_union_constraint(t)
            })? {
                query.traversed_inert_arm = true;
            }
            return Ok(ty);
        }
        self.get_narrowable_type_for_reference(ty, query.reference, CheckMode::NORMAL)
    }

    /// tsc-port: getInitialType @6.0.3
    /// tsc-hash: 63033c68f243136b5462ad09aaf856d03ec8fd66970c852a4e68c3ad3e30b8b1
    /// tsc-span: _tsc.js:69905-69907
    fn get_initial_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if self.kind_of(node) == SyntaxKind::VariableDeclaration {
            self.get_initial_type_of_variable_declaration(node)
        } else {
            self.get_initial_type_of_binding_element(node)
        }
    }

    /// tsc-port: getInitialTypeOfVariableDeclaration @6.0.3
    /// tsc-hash: 5c0b40fb58b0730526a9d0f16bc128e96ad5aec0ac305aadabe5d69de62515a0
    /// tsc-span: _tsc.js:69893-69904
    fn get_initial_type_of_variable_declaration(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let initializer = match self.data_of(node) {
            NodeData::VariableDeclaration(data) => data.initializer,
            _ => None,
        };
        if let Some(initializer) = initializer {
            return self.get_type_of_initializer(initializer);
        }
        let grand = self
            .parent_of(node)
            .and_then(|parent| self.parent_of(parent));
        match grand.map(|grand| self.kind_of(grand)) {
            Some(SyntaxKind::ForInStatement) => Ok(self.tables.intrinsics.string),
            Some(SyntaxKind::ForOfStatement) => {
                self.check_right_hand_side_of_for_of(grand.expect("matched Some above"))
            }
            _ => Ok(self.tables.intrinsics.error),
        }
    }

    // (getTypeOfInitializer lives in functions.rs — the 5.x
    // definite-assignment family landed it; consumed as-is.)

    /// tsc-port: getInitialTypeOfBindingElement @6.0.3
    /// tsc-hash: 82b8468d8756e72241395057d7afda23261a7eaf66b366e487a69a7c69d65c04
    /// tsc-span: _tsc.js:69883-69888
    fn get_initial_type_of_binding_element(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let Some(pattern) = self.parent_of(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        let Some(pattern_parent) = self.parent_of(pattern) else {
            return Ok(self.tables.intrinsics.error);
        };
        let parent_type = self.get_initial_type(pattern_parent)?;
        let (property_name, name, dot_dot_dot, initializer) = match self.data_of(node) {
            NodeData::BindingElement(data) => (
                data.property_name,
                data.name,
                data.dot_dot_dot_token.is_some(),
                data.initializer,
            ),
            _ => (None, None, false, None),
        };
        let ty = if self.kind_of(pattern) == SyntaxKind::ObjectBindingPattern {
            let Some(name) = property_name.or(name) else {
                return Ok(self.tables.intrinsics.error);
            };
            self.get_type_of_destructured_property(parent_type, name)?
        } else if !dot_dot_dot {
            let elements = match self.data_of(pattern) {
                NodeData::ArrayBindingPattern(data) => data.elements,
                NodeData::ObjectBindingPattern(data) => data.elements,
                _ => None,
            };
            let index = elements
                .map(|elements| self.binder.node_array(elements).nodes.clone())
                .and_then(|nodes| nodes.iter().position(|&element| element == node));
            let Some(index) = index else {
                return Ok(self.tables.intrinsics.error);
            };
            self.get_type_of_destructured_array_element(parent_type, index)?
        } else {
            self.get_type_of_destructured_spread_expression(parent_type)?
        };
        self.get_type_with_default(ty, initializer)
    }

    /// tsc-port: getAssignedType @6.0.3
    /// tsc-hash: b4b5f7b8b74b9996686651b4ad2a673a3725780eac0b37b285203430a9074a34
    /// tsc-span: _tsc.js:69861-69882
    fn get_assigned_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        match self.kind_of(parent) {
            SyntaxKind::ForInStatement => Ok(self.tables.intrinsics.string),
            SyntaxKind::ForOfStatement => self.check_right_hand_side_of_for_of(parent),
            SyntaxKind::BinaryExpression => self.get_assigned_type_of_binary_expression(parent),
            SyntaxKind::DeleteExpression => Ok(self.tables.intrinsics.undefined),
            SyntaxKind::ArrayLiteralExpression => {
                self.get_assigned_type_of_array_literal_element(parent, node)
            }
            SyntaxKind::SpreadElement => self.get_assigned_type_of_spread_expression(parent),
            SyntaxKind::PropertyAssignment => self.get_assigned_type_of_property_assignment(parent),
            SyntaxKind::ShorthandPropertyAssignment => {
                self.get_assigned_type_of_shorthand_property_assignment(parent)
            }
            _ => Ok(self.tables.intrinsics.error),
        }
    }

    /// tsc-port: getAssignedTypeOfBinaryExpression @6.0.3
    /// tsc-hash: 6e0d133270223aacd7bb19fb670e68264e38741db9a130f01e77113ecb5ada3d
    /// tsc-span: _tsc.js:69842-69845
    fn get_assigned_type_of_binary_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let parent = self.parent_of(node);
        let is_destructuring_default_assignment = match parent.map(|parent| self.kind_of(parent)) {
            Some(SyntaxKind::ArrayLiteralExpression) => {
                self.is_destructuring_assignment_target(parent.expect("matched Some above"))
            }
            Some(SyntaxKind::PropertyAssignment) => {
                let grand = parent.and_then(|parent| self.parent_of(parent));
                grand.is_some_and(|grand| self.is_destructuring_assignment_target(grand))
            }
            _ => false,
        };
        let right = match self.data_of(node) {
            NodeData::BinaryExpression(data) => data.right,
            _ => None,
        };
        let Some(right) = right else {
            return Ok(self.tables.intrinsics.error);
        };
        if is_destructuring_default_assignment {
            let assigned = self.get_assigned_type(node)?;
            self.get_type_with_default(assigned, Some(right))
        } else {
            self.get_type_of_expression(right)
        }
    }

    // (isDestructuringAssignmentTarget lives in expr.rs — consumed
    // as-is.)

    /// tsc-port: getAssignedTypeOfArrayLiteralElement @6.0.3
    /// tsc-hash: 81098c3c6834de6d1de1ff1195bd70b934988966adfa1864ae6f331ccfe03fda
    /// tsc-span: _tsc.js:69849-69851
    fn get_assigned_type_of_array_literal_element(
        &mut self,
        node: NodeId,
        element: NodeId,
    ) -> CheckResult2<TypeId> {
        let elements = match self.data_of(node) {
            NodeData::ArrayLiteralExpression(data) => data.elements,
            _ => None,
        };
        let index = elements
            .map(|elements| self.binder.node_array(elements).nodes.clone())
            .and_then(|nodes| nodes.iter().position(|&e| e == element));
        let Some(index) = index else {
            return Ok(self.tables.intrinsics.error);
        };
        let assigned = self.get_assigned_type(node)?;
        self.get_type_of_destructured_array_element(assigned, index)
    }

    /// tsc-port: getAssignedTypeOfSpreadExpression @6.0.3
    /// tsc-hash: e93d0e7815a6cd70a1eb5b2205b04f19c38e2564059de6fcf55e4b3e8e64c601
    /// tsc-span: _tsc.js:69852-69854
    fn get_assigned_type_of_spread_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        let assigned = self.get_assigned_type(parent)?;
        self.get_type_of_destructured_spread_expression(assigned)
    }

    /// tsc-port: getAssignedTypeOfPropertyAssignment @6.0.3
    /// tsc-hash: 1244de4b452537a0da4b8aa1c154266e9e8df9afc185d59c45ea739dbdb0d6b6
    /// tsc-span: _tsc.js:69855-69857
    fn get_assigned_type_of_property_assignment(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        let name = match self.data_of(node) {
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            _ => None,
        };
        let Some(name) = name else {
            return Ok(self.tables.intrinsics.error);
        };
        let assigned = self.get_assigned_type(parent)?;
        self.get_type_of_destructured_property(assigned, name)
    }

    /// tsc-port: getAssignedTypeOfShorthandPropertyAssignment @6.0.3
    /// tsc-hash: c048892f3d80b805148217d97d9c203adb1822c18a5c8bc555f35abb9affb571
    /// tsc-span: _tsc.js:69858-69860
    fn get_assigned_type_of_shorthand_property_assignment(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        let ty = self.get_assigned_type_of_property_assignment(node)?;
        let initializer = match self.data_of(node) {
            NodeData::ShorthandPropertyAssignment(data) => data.object_assignment_initializer,
            _ => None,
        };
        self.get_type_with_default(ty, initializer)
    }

    /// tsc-port: getTypeOfDestructuredProperty @6.0.3
    /// tsc-hash: b326b9dbfb5842004b39a1ff1fa66676ee9e5a8b507ee9a9f1bba7e56a5e2c0f
    /// tsc-span: _tsc.js:69813-69819
    fn get_type_of_destructured_property(
        &mut self,
        ty: TypeId,
        name: NodeId,
    ) -> CheckResult2<TypeId> {
        let name_type = self.get_literal_type_from_property_name(name)?;
        let Some(text) = self.property_name_from_type_usable(name_type) else {
            return Ok(self.tables.intrinsics.error);
        };
        if let Some(prop_type) = self.get_type_of_property_of_type(ty, &text)? {
            return Ok(prop_type);
        }
        let index_type = self.get_applicable_index_info_for_name(ty, &text)?;
        let included = self.include_undefined_in_index_signature(index_type)?;
        Ok(included.unwrap_or(self.tables.intrinsics.error))
    }

    /// tsc-port: getTypeOfDestructuredArrayElement @6.0.3
    /// tsc-hash: 8c9781fd85428bafe766792b3ffa3a80e5e3eec826bf0dbfd4860fa83aa119e6
    /// tsc-span: _tsc.js:69820-69828
    fn get_type_of_destructured_array_element(
        &mut self,
        ty: TypeId,
        index: usize,
    ) -> CheckResult2<TypeId> {
        if self.every_type_is_tuple_like(ty)? {
            if let Some(element) = self.get_tuple_element_type_for_flow(ty, index)? {
                return Ok(element);
            }
        }
        let undefined = self.tables.intrinsics.undefined;
        let iterated = self.check_iterated_type_or_element_type(
            tsrs2_types::IterationUse::DESTRUCTURING,
            ty,
            undefined,
            /*error_node*/ None,
        )?;
        let included = self.include_undefined_in_index_signature(Some(iterated))?;
        Ok(included.unwrap_or(self.tables.intrinsics.error))
    }

    /// The everyType(type, isTupleLikeType) composition — the union
    /// traversal with a fallible predicate (constraints.rs every_type
    /// takes an infallible one).
    /// tsrs-native: fallible-everyType instance.
    fn every_type_is_tuple_like(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let members: Vec<TypeId> = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            }
        } else {
            vec![ty]
        };
        for member in members {
            if !self.is_tuple_like_type(member)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: getTupleElementType @6.0.3
    /// tsc-hash: ef850f27e6c5dbe9558b5edb4dfefe99f3f78ee1f7eaa8d04549fd5b092c579f
    /// tsc-span: _tsc.js:67729-67738
    fn get_tuple_element_type_for_flow(
        &mut self,
        ty: TypeId,
        index: usize,
    ) -> CheckResult2<Option<TypeId>> {
        let prop_type = self.get_type_of_property_of_type(ty, &index.to_string())?;
        if prop_type.is_some() {
            return Ok(prop_type);
        }
        if self.every_type(ty, |state, t| state.tables.is_tuple_type(t)) {
            let undefined_or_missing = self
                .options
                .no_unchecked_indexed_access
                .unwrap_or(false)
                .then_some(self.tables.intrinsics.undefined);
            return Ok(Some(self.get_tuple_element_type_out_of_start_count(
                ty,
                index,
                undefined_or_missing,
            )?));
        }
        Ok(None)
    }

    /// tsc-port: getTypeOfDestructuredSpreadExpression @6.0.3
    /// tsc-hash: d0d427e2d9e3fd54b82e148d1cb828841e27fb2904880c204460f11a9183c9a9
    /// tsc-span: _tsc.js:69833-69841
    fn get_type_of_destructured_spread_expression(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let undefined = self.tables.intrinsics.undefined;
        let iterated = self.check_iterated_type_or_element_type(
            tsrs2_types::IterationUse::DESTRUCTURING,
            ty,
            undefined,
            /*error_node*/ None,
        )?;
        self.create_array_type(iterated, /*readonly*/ false)
    }

    /// tsc-port: getTypeWithDefault @6.0.3
    /// tsc-hash: c4b4fcb6572b5b2c45c647b0069654038cb5e114051e6dc0603a205e9e58fc1c
    /// tsc-span: _tsc.js:69810-69812
    fn get_type_with_default(
        &mut self,
        ty: TypeId,
        default_expression: Option<NodeId>,
    ) -> CheckResult2<TypeId> {
        let Some(default_expression) = default_expression else {
            return Ok(ty);
        };
        let non_undefined = self.get_non_undefined_type(ty)?;
        let default_type = self.get_type_of_expression(default_expression)?;
        self.get_union_type_ex(
            &[non_undefined, default_type],
            tsrs2_types::UnionReduction::Literal,
        )
    }

    // ---- the assignment-reduced-type rule ----

    /// tsc-port: getAssignmentReducedType @6.0.3
    /// tsc-hash: 9c2e79d09c42d5e9a69ef07cc7808ea2716a843a2d049b1f61265e17fc7f09b9
    /// tsc-span: _tsc.js:69675-69684
    fn get_assignment_reduced_type(
        &mut self,
        declared: TypeId,
        assigned: TypeId,
    ) -> CheckResult2<TypeId> {
        if declared == assigned {
            return Ok(declared);
        }
        if self.tables.flags_of(assigned).intersects(TypeFlags::NEVER) {
            return Ok(assigned);
        }
        let key = format!("A{},{}", declared.0, assigned.0);
        if let Some(cached) = self.get_cached_type(&key) {
            return Ok(cached);
        }
        let reduced = self.get_assignment_reduced_type_worker(declared, assigned)?;
        Ok(self.set_cached_type(key, reduced))
    }

    /// tsc-port: getAssignmentReducedTypeWorker @6.0.3
    /// tsc-hash: c70539bf7e2c687bfb647f0e23e68d527b658f555794d2b1335ab76c899a2b0e
    /// tsc-span: _tsc.js:69685-69689
    fn get_assignment_reduced_type_worker(
        &mut self,
        declared: TypeId,
        assigned: TypeId,
    ) -> CheckResult2<TypeId> {
        let filtered = self.filter_type_with(declared, |state, t| {
            state.type_maybe_assignable_to(assigned, t)
        })?;
        let fresh_boolean = self
            .tables
            .flags_of(assigned)
            .intersects(TypeFlags::BOOLEAN_LITERAL)
            && self.tables.is_fresh_literal_type(assigned);
        let reduced = if fresh_boolean {
            self.map_type(
                filtered,
                &mut |state, t| Ok(Some(state.tables.get_fresh_type_of_literal_type(t))),
                /*no_reductions*/ false,
            )?
            .unwrap_or(filtered)
        } else {
            filtered
        };
        if self.is_type_assignable_to(assigned, reduced)? {
            Ok(reduced)
        } else {
            Ok(declared)
        }
    }

    /// tsc-port: typeMaybeAssignableTo @6.0.3
    /// tsc-hash: 28e858bc4a5c15238844526ffeaeb89e515bf1abf827546d148919d9cc33c71c
    /// tsc-span: _tsc.js:69664-69674
    fn type_maybe_assignable_to(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        if !self.tables.flags_of(source).intersects(TypeFlags::UNION) {
            return self.is_type_assignable_to(source, target);
        }
        let members: Vec<TypeId> = match &self.tables.type_of(source).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies union data"),
        };
        for member in members {
            if self.is_type_assignable_to(member, target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- reference containment + candidates ----

    /// tsc-port: containsMatchingReference @6.0.3
    /// tsc-hash: 792f946d12e93e4bda7e12ed10bd21472ebfa560d5a58ca4d3798e674475d22c
    /// tsc-span: _tsc.js:69544-69552
    fn contains_matching_reference(
        &mut self,
        source: NodeId,
        target: NodeId,
    ) -> CheckResult2<bool> {
        let mut source = source;
        while matches!(
            self.kind_of(source),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            let Some(expression) = self.access_expression_of(source) else {
                return Ok(false);
            };
            source = expression;
            if self.is_matching_reference(source, target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: optionalChainContainsReference @6.0.3
    /// tsc-hash: 65b554fad85a6151c57fa295260765fcf8a0e52832d14a47c42a10e076d4205c
    /// tsc-span: _tsc.js:69553-69561
    pub(crate) fn optional_chain_contains_reference(
        &mut self,
        source: NodeId,
        target: NodeId,
    ) -> CheckResult2<bool> {
        let mut source = source;
        loop {
            let file_source = self.binder.source_of_node(source);
            if !node_util::is_optional_chain(file_source, source) {
                return Ok(false);
            }
            let next = match self.data_of(source) {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::CallExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next else {
                return Ok(false);
            };
            source = next;
            if self.is_matching_reference(source, target)? {
                return Ok(true);
            }
        }
    }

    /// tsc-port: getReferenceCandidate @6.0.3
    /// tsc-hash: a2fb9d33d108f37f4cc17852884a58bc8ff23e81302c72d6c17d16f274694f3c
    /// tsc-span: _tsc.js:69911-69927
    pub(crate) fn get_reference_candidate(&self, node: NodeId) -> NodeId {
        match self.data_of(node) {
            NodeData::ParenthesizedExpression(data) => {
                let Some(inner) = data.expression else {
                    return node;
                };
                self.get_reference_candidate(inner)
            }
            NodeData::BinaryExpression(data) => {
                let (left, right, operator) = (data.left, data.right, data.operator_token);
                let Some(operator) = operator else {
                    return node;
                };
                match self.kind_of(operator) {
                    SyntaxKind::EqualsToken
                    | SyntaxKind::BarBarEqualsToken
                    | SyntaxKind::AmpersandAmpersandEqualsToken
                    | SyntaxKind::QuestionQuestionEqualsToken => match left {
                        Some(left) => self.get_reference_candidate(left),
                        None => node,
                    },
                    SyntaxKind::CommaToken => match right {
                        Some(right) => self.get_reference_candidate(right),
                        None => node,
                    },
                    _ => node,
                }
            }
            _ => node,
        }
    }

    /// tsc-port: getReferenceRoot @6.0.3
    /// tsc-hash: 20b35236e728c6c99463ecf9e08b756b5bdd371bea4cddde41072c860d2a6fb5
    /// tsc-span: _tsc.js:69928-69931
    fn get_reference_root(&self, node: NodeId) -> NodeId {
        let Some(parent) = self.parent_of(node) else {
            return node;
        };
        let hop = match self.data_of(parent) {
            NodeData::ParenthesizedExpression(_) => true,
            NodeData::BinaryExpression(data) => {
                let (left, right, operator) = (data.left, data.right, data.operator_token);
                match operator.map(|operator| self.kind_of(operator)) {
                    Some(SyntaxKind::EqualsToken) => left == Some(node),
                    Some(SyntaxKind::CommaToken) => right == Some(node),
                    _ => false,
                }
            }
            _ => false,
        };
        if hop {
            self.get_reference_root(parent)
        } else {
            node
        }
    }

    // ---- assignment-shape predicates ----

    /// tsc-port: isEmptyArrayAssignment @6.0.3
    /// tsc-hash: 0c0f3358c610a8cb1df1fb9e6b9ec4978a9f7eab447a635a34840ba1b5953483
    /// tsc-span: _tsc.js:69908-69910
    fn is_empty_array_assignment(&self, node: NodeId) -> bool {
        if self.kind_of(node) == SyntaxKind::VariableDeclaration {
            let initializer = match self.data_of(node) {
                NodeData::VariableDeclaration(data) => data.initializer,
                _ => None,
            };
            return initializer
                .is_some_and(|initializer| self.is_empty_array_literal_expr(initializer));
        }
        if self.kind_of(node) == SyntaxKind::BindingElement {
            return false;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        let right = match self.data_of(parent) {
            NodeData::BinaryExpression(data) => data.right,
            _ => None,
        };
        right.is_some_and(|right| self.is_empty_array_literal_expr(right))
    }

    /// tsc-port: isVarConstLike @6.0.3
    /// tsc-hash: c0803a735166c3246d683c39c34758f3f64d3bbc662a7d99ae6b7673538eb414
    /// tsc-span: _tsc.js:90557-90560
    fn is_var_const_like(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let block_scope_kind = node_util::get_combined_node_flags(source, node).bits()
            & (NodeFlags::LET.bits() | NodeFlags::CONST.bits() | NodeFlags::USING.bits());
        block_scope_kind == NodeFlags::CONST.bits()
            || block_scope_kind == NodeFlags::USING.bits()
            || block_scope_kind == NodeFlags::AWAIT_USING.bits()
    }

    // ---- evolving (auto) arrays ----

    /// tsc-port: createEvolvingArrayType @6.0.3
    /// tsc-hash: e2b07f4bd3fcbc2966f008c395f9287c6f5761711cf2fff52b24ad4194d7b3b3
    /// tsc-span: _tsc.js:70073-70077
    fn create_evolving_array_type(&mut self, element_type: TypeId) -> TypeId {
        let id = self
            .tables
            .create_type(TypeFlags::OBJECT, TypeData::EvolvingArray { element_type });
        self.tables.type_mut(id).object_flags = tsrs2_types::ObjectFlags::EVOLVING_ARRAY;
        id
    }

    /// tsc-port: getEvolvingArrayType @6.0.3
    /// tsc-hash: e149735ee643cecc431c10f545d8809562de0b184eedfdded03f1aa112fa8e4d
    /// tsc-span: _tsc.js:70078-70080
    pub(crate) fn get_evolving_array_type(&mut self, element_type: TypeId) -> TypeId {
        if let Some(&cached) = self.evolving_array_types.get(&element_type) {
            return cached;
        }
        let created = self.create_evolving_array_type(element_type);
        self.evolving_array_types.insert(element_type, created);
        created
    }

    /// tsc-port: addEvolvingArrayElementType @6.0.3
    /// tsc-hash: 32a4fd3e4e26bfd2e736d04db7d89cbbe155c3cf49c02f7573eecc8b9fd2e38b
    /// tsc-span: _tsc.js:70081-70084
    fn add_evolving_array_element_type(
        &mut self,
        evolving: TypeId,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        let context_free = self.get_context_free_type_of_expression(node)?;
        let base = self.get_base_type_of_literal_type(context_free)?;
        let element_type = self.get_regular_type_of_object_literal(base)?;
        let TypeData::EvolvingArray {
            element_type: current,
        } = self.tables.type_of(evolving).data
        else {
            unreachable!("add_evolving_array_element_type takes an evolving array");
        };
        if self.is_type_subset_of(element_type, current)? {
            return Ok(evolving);
        }
        let union = self.get_union_type_ex(
            &[current, element_type],
            tsrs2_types::UnionReduction::Literal,
        )?;
        Ok(self.get_evolving_array_type(union))
    }

    /// tsc-port: createFinalArrayType @6.0.3
    /// tsc-hash: 38e822f7ad2f07312da4c233c5a17c089b61164ad3b8f2cbaae7e0e4cbb99b69
    /// tsc-span: _tsc.js:70085-70089
    fn create_final_array_type(&mut self, element_type: TypeId) -> CheckResult2<TypeId> {
        if self
            .tables
            .flags_of(element_type)
            .intersects(TypeFlags::NEVER)
        {
            return self.auto_array_type();
        }
        let element = if self
            .tables
            .flags_of(element_type)
            .intersects(TypeFlags::UNION)
        {
            let members: Vec<TypeId> = match &self.tables.type_of(element_type).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            self.get_union_type_ex(&members, tsrs2_types::UnionReduction::Subtype)?
        } else {
            element_type
        };
        self.create_array_type(element, /*readonly*/ false)
    }

    /// tsc-port: getFinalArrayType @6.0.3
    /// tsc-hash: 5de7069cbb107051b983ecbefcd3d6e654c367fc7b19efe74f11fec3ebb767af
    /// tsc-span: _tsc.js:70090-70092
    fn get_final_array_type(&mut self, evolving: TypeId) -> CheckResult2<TypeId> {
        if let Some(&cached) = self.final_array_types.get(&evolving) {
            return Ok(cached);
        }
        let TypeData::EvolvingArray { element_type } = self.tables.type_of(evolving).data else {
            unreachable!("get_final_array_type takes an evolving array");
        };
        let created = self.create_final_array_type(element_type)?;
        self.final_array_types.insert(evolving, created);
        Ok(created)
    }

    /// tsc-port: finalizeEvolvingArrayType @6.0.3
    /// tsc-hash: 478346dca64679cb33da37deb844a147fcacbc763ccdc822558e1ecfcfca6901
    /// tsc-span: _tsc.js:70093-70095
    pub(crate) fn finalize_evolving_array_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(tsrs2_types::ObjectFlags::EVOLVING_ARRAY)
        {
            self.get_final_array_type(ty)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getElementTypeOfEvolvingArrayType @6.0.3
    /// tsc-hash: 751aa5f1b4fa3e3c9e832085dddee81f03359bbfaa568d3e7bcf1a6391d2d94a
    /// tsc-span: _tsc.js:70096-70098
    pub(crate) fn get_element_type_of_evolving_array_type(&self, ty: TypeId) -> TypeId {
        if let TypeData::EvolvingArray { element_type } = self.tables.type_of(ty).data {
            element_type
        } else {
            self.tables.intrinsics.never
        }
    }

    /// tsc-port: isEvolvingArrayTypeList @6.0.3
    /// tsc-hash: b93ff435c92bb04a9225f07ba1f069517107c5a8d42957d9bbe12ddc7945c4f4
    /// tsc-span: _tsc.js:70099-70110
    pub(crate) fn is_evolving_array_type_list(&self, types: &[TypeId]) -> bool {
        let mut has_evolving_array_type = false;
        for &ty in types {
            if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
                continue;
            }
            if !self
                .tables
                .object_flags_of(ty)
                .intersects(tsrs2_types::ObjectFlags::EVOLVING_ARRAY)
            {
                return false;
            }
            has_evolving_array_type = true;
        }
        has_evolving_array_type
    }

    /// tsc-port: isEvolvingArrayOperationTarget @6.0.3
    /// tsc-hash: 952c9cdadcc0204fcaa726dfc86ec9b7259039a58ab7349b77718a4e145f0875
    /// tsc-span: _tsc.js:70111-70117
    pub(crate) fn is_evolving_array_operation_target(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<bool> {
        let root = self.get_reference_root(node);
        let Some(parent) = self.parent_of(root) else {
            return Ok(false);
        };
        if self.kind_of(parent) == SyntaxKind::PropertyAccessExpression {
            let name = match self.data_of(parent) {
                NodeData::PropertyAccessExpression(data) => data.name,
                _ => None,
            };
            let name_is_identifier =
                name.is_some_and(|name| self.kind_of(name) == SyntaxKind::Identifier);
            let name_text = self.escaped_text_of(name).map(str::to_owned);
            let grand_is_call = self
                .parent_of(parent)
                .is_some_and(|grand| self.kind_of(grand) == SyntaxKind::CallExpression);
            // isPushOrUnshiftIdentifier (15983) folded into the match.
            let is_length_push_or_unshift = name_text.as_deref() == Some("length")
                || (grand_is_call
                    && name_is_identifier
                    && matches!(name_text.as_deref(), Some("push") | Some("unshift")));
            if is_length_push_or_unshift {
                return Ok(true);
            }
        }
        if self.kind_of(parent) == SyntaxKind::ElementAccessExpression {
            let (expression, argument) = match self.data_of(parent) {
                NodeData::ElementAccessExpression(data) => {
                    (data.expression, data.argument_expression)
                }
                _ => (None, None),
            };
            if expression == Some(root) {
                let Some(grand) = self.parent_of(parent) else {
                    return Ok(false);
                };
                let (left, operator) = match self.data_of(grand) {
                    NodeData::BinaryExpression(data) => (data.left, data.operator_token),
                    _ => (None, None),
                };
                if operator
                    .is_some_and(|operator| self.kind_of(operator) == SyntaxKind::EqualsToken)
                    && left == Some(parent)
                    && self.get_assignment_target_kind(grand) == crate::expr::AssignmentKind::None
                {
                    if let Some(argument) = argument {
                        let index_type = self.get_type_of_expression(argument)?;
                        return self.is_type_assignable_to_kind(
                            index_type,
                            TypeFlags::NUMBER_LIKE,
                            /*strict*/ false,
                        );
                    }
                }
            }
        }
        Ok(false)
    }

    // ---- declared-type optionality (the checkIdentifier ladder) ----

    /// tsc-port: removeOptionalityFromDeclaredType @6.0.3
    /// tsc-hash: bf6f7efd9a22cf9d77a9e882414496b8dafc82e6dd53d3307c400084a7d1103c
    /// tsc-span: _tsc.js:71618-71621
    pub(crate) fn remove_optionality_from_declared_type(
        &mut self,
        declared_type: TypeId,
        declaration: NodeId,
    ) -> CheckResult2<TypeId> {
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let has_initializer = matches!(
            self.data_of(declaration),
            NodeData::Parameter(data) if data.initializer.is_some()
        );
        let remove_undefined = strict_null_checks
            && self.kind_of(declaration) == SyntaxKind::Parameter
            && has_initializer
            && self.has_type_facts(declared_type, tsrs2_types::TypeFacts::IS_UNDEFINED)?
            && !self.parameter_initializer_contains_undefined(declaration)?;
        if remove_undefined {
            self.get_type_with_facts(declared_type, tsrs2_types::TypeFacts::NE_UNDEFINED)
        } else {
            Ok(declared_type)
        }
    }

    /// tsc-port: parameterInitializerContainsUndefined @6.0.3
    /// tsc-hash: 63964a337ce0197adec292e2db7bdd00b0ce4cdf7033fb39ddecc833bd35cb35
    /// tsc-span: _tsc.js:71602-71617
    ///
    /// tsc cannot fail mid-resolution; ours can unwind
    /// (checkDeclarationInitializer), so the pushed resolution entry
    /// pops on the Err path too (the unwind invariant).
    fn parameter_initializer_contains_undefined(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<bool> {
        if let Some(cached) = self
            .links
            .node(declaration)
            .parameter_initializer_contains_undefined
        {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Node(declaration),
            tsrs2_types::TypeSystemPropertyName::PARAMETER_INITIALIZER_CONTAINS_UNDEFINED,
        ) {
            let symbol = self.get_symbol_of_declaration(declaration)?;
            self.report_circularity_error(symbol);
            return Ok(true);
        }
        let initializer_type =
            match self.check_declaration_initializer(declaration, CheckMode::NORMAL, None) {
                Ok(ty) => ty,
                Err(unwind) => {
                    self.pop_type_resolution();
                    return Err(unwind);
                }
            };
        let contains =
            match self.has_type_facts(initializer_type, tsrs2_types::TypeFacts::IS_UNDEFINED) {
                Ok(contains) => contains,
                Err(unwind) => {
                    self.pop_type_resolution();
                    return Err(unwind);
                }
            };
        if !self.pop_type_resolution() {
            let symbol = self.get_symbol_of_declaration(declaration)?;
            self.report_circularity_error(symbol);
            return Ok(true);
        }
        if self
            .links
            .node(declaration)
            .parameter_initializer_contains_undefined
            .is_none()
        {
            self.links
                .set_node_parameter_initializer_contains_undefined(
                    self.speculation_depth,
                    declaration,
                    contains,
                );
        }
        Ok(contains)
    }

    // ---- isMatchingReference — narrowing's identity gate ----
}

/// The DESTRUCTURING FLOW ENTRY (6.4b): tsc resolves the type of a
/// destructured element by flow-querying a parse-node-factory
/// synthetic access chain `base["p0"]…` carrying the base access's
/// flowNode. The checker cannot allocate nodes, so the chain is query
/// DATA (`FlowQuery::synthetic_props`) and every reference-shaped
/// probe of the walk goes through the `*_query_reference` wrappers
/// below.
impl<'a> CheckerState<'a> {
    /// tsc-port: getFlowTypeOfDestructuring @6.0.3
    /// tsc-hash: 59207005b9e8dbd8b079ce8e0f0701387c7c0c46c2a46dbe860407759dea31ac
    /// tsc-span: _tsc.js:55892-55895
    ///
    /// Live since 6.4b (the [FLOW M5] identity stub retired with the
    /// narrowers): no synthesizable reference means the declared type,
    /// exactly tsc's fallback.
    pub(crate) fn get_flow_type_of_destructuring(
        &mut self,
        node: NodeId,
        declared_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let Some((base, props)) = self.synthetic_element_access_chain(node)? else {
            return Ok(declared_type);
        };
        let flow_node = self.flow_node_of(base);
        self.get_flow_type_of_reference_full(
            base,
            Some(props),
            declared_type,
            declared_type,
            None,
            flow_node,
        )
    }

    /// tsc-port: getSyntheticElementAccess @6.0.3
    /// tsc-hash: 91b4a90678ee5656dc24899f0ca95842e3a91a02fd98e8af1fee89db006bb8d0
    /// tsc-span: _tsc.js:55896-55913
    ///
    /// Returns the chain encoding of tsc's synthetic node: the real
    /// BASE expression plus the accessed-name path (outermost last,
    /// escaped — the factory's string literal reads back through
    /// getAccessedPropertyName as an escaped name). tsc gates on the
    /// parent access carrying a flowNode; nested synthetic parents
    /// inherit the base's, so the chain gate is flow_node_of(base) —
    /// checked per level here (a REAL parent access without a flow
    /// node bails exactly like tsc's canHaveFlowNode miss).
    fn synthetic_element_access_chain(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<(NodeId, Vec<String>)>> {
        let Some((base, mut props)) = self.parent_element_access_chain(node)? else {
            return Ok(None);
        };
        if self.flow_node_of(base).is_none() {
            return Ok(None);
        }
        let Some(prop_name) = self.get_destructuring_property_name(node)? else {
            return Ok(None);
        };
        // tsc 55901 `if (propName)`: the EMPTY text (`const {"": x}`)
        // is falsy — no synthetic access, declared type.
        if prop_name.is_empty() {
            return Ok(None);
        }
        props.push(tsrs2_syntax::escape_leading_underscores(&prop_name));
        Ok(Some((base, props)))
    }

    /// tsc-port: getParentElementAccess @6.0.3
    /// tsc-hash: abde7d7ff460d0e6b275661e9a4ab27f2aeed2c63fbf60b56b4ba926e66e4b30
    /// tsc-span: _tsc.js:55914-55927
    ///
    /// The parent-access chain of a destructuring element: nested
    /// binding elements / property assignments recurse (another
    /// synthetic level), an enclosing array literal recurses on the
    /// literal itself, and the roots are the declaration initializer
    /// or the assignment RHS (real nodes, empty chain).
    fn parent_element_access_chain(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<(NodeId, Vec<String>)>> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        let Some(ancestor) = self.parent_of(parent) else {
            return Ok(None);
        };
        match self.kind_of(ancestor) {
            SyntaxKind::BindingElement | SyntaxKind::PropertyAssignment => {
                self.synthetic_element_access_chain(ancestor)
            }
            SyntaxKind::ArrayLiteralExpression => self.synthetic_element_access_chain(parent),
            SyntaxKind::VariableDeclaration => {
                let initializer = match self.data_of(ancestor) {
                    NodeData::VariableDeclaration(data) => data.initializer,
                    _ => None,
                };
                Ok(initializer.map(|initializer| (initializer, Vec::new())))
            }
            SyntaxKind::BinaryExpression => {
                let right = match self.data_of(ancestor) {
                    NodeData::BinaryExpression(data) => data.right,
                    _ => None,
                };
                Ok(right.map(|right| (right, Vec::new())))
            }
            _ => Ok(None),
        }
    }

    /// tsc isMatchingReference with the QUERY's reference as source —
    /// the single entry every walk/narrower site uses, so a synthetic
    /// destructuring chain matches exactly like tsc's synthetic node.
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn is_matching_query_reference(
        &mut self,
        query: &FlowQuery,
        target: NodeId,
    ) -> CheckResult2<bool> {
        match &query.synthetic_props {
            None => self.is_matching_reference(query.reference, target),
            Some(props) => {
                let props = props.clone();
                self.is_matching_synthetic_chain(query.reference, &props, target)
            }
        }
    }

    /// tsc isMatchingReference (69448) with source = the synthetic
    /// chain `base[p0]…[pn]`: the target-side unwrap arms mirror the
    /// plain port; the source access arm compares the outermost chain
    /// name against the target's accessed name and recurses on the
    /// receivers (an empty chain IS the base — plain matching).
    /// tsrs-native: chain-encoded source arm of the same tsc span.
    fn is_matching_synthetic_chain(
        &mut self,
        base: NodeId,
        props: &[String],
        target: NodeId,
    ) -> CheckResult2<bool> {
        let Some((outer, receiver_props)) = props.split_last() else {
            return self.is_matching_reference(base, target);
        };
        match self.kind_of(target) {
            SyntaxKind::ParenthesizedExpression | SyntaxKind::NonNullExpression => {
                let inner = match self.data_of(target) {
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    NodeData::NonNullExpression(data) => data.expression,
                    _ => None,
                };
                match inner {
                    Some(inner) => self.is_matching_synthetic_chain(base, props, inner),
                    None => Ok(false),
                }
            }
            SyntaxKind::BinaryExpression => {
                if let NodeData::BinaryExpression(data) = self.data_of(target) {
                    let (left, right, operator) = (data.left, data.right, data.operator_token);
                    if let (Some(left), Some(operator)) = (left, operator) {
                        if node_util::is_assignment_operator(self.kind_of(operator))
                            && self.is_matching_synthetic_chain(base, props, left)?
                        {
                            return Ok(true);
                        }
                    }
                    if let (Some(right), Some(operator)) = (right, operator) {
                        if self.kind_of(operator) == SyntaxKind::CommaToken
                            && self.is_matching_synthetic_chain(base, props, right)?
                        {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                let Some(target_name) = self.get_accessed_property_name(target)? else {
                    return Ok(false);
                };
                if &target_name != outer {
                    return Ok(false);
                }
                let receiver = match self.data_of(target) {
                    NodeData::PropertyAccessExpression(data) => data.expression,
                    NodeData::ElementAccessExpression(data) => data.expression,
                    _ => None,
                };
                match receiver {
                    Some(receiver) => {
                        self.is_matching_synthetic_chain(base, receiver_props, receiver)
                    }
                    None => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }

    /// tsc containsMatchingReference with the QUERY's reference as
    /// source: the synthetic chain strips one level at a time (each
    /// prefix is "contained"), then continues into the real base.
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn contains_matching_query_reference(
        &mut self,
        query: &FlowQuery,
        target: NodeId,
    ) -> CheckResult2<bool> {
        let Some(props) = query.synthetic_props.clone() else {
            return self.contains_matching_reference(query.reference, target);
        };
        for len in (1..props.len()).rev() {
            if self.is_matching_synthetic_chain(query.reference, &props[..len], target)? {
                return Ok(true);
            }
        }
        if self.is_matching_reference(query.reference, target)? {
            return Ok(true);
        }
        self.contains_matching_reference(query.reference, target)
    }

    /// tsc getAccessedPropertyName(reference), synthetic-aware: the
    /// chain's outermost name IS the accessed name of tsc's factory
    /// node.
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn query_reference_accessed_property_name(
        &mut self,
        query: &FlowQuery,
    ) -> CheckResult2<Option<String>> {
        match &query.synthetic_props {
            Some(props) => Ok(props.last().cloned()),
            None => self.get_accessed_property_name(query.reference),
        }
    }

    /// tsc isAccessExpression(reference), synthetic-aware (the
    /// factory node is an ElementAccessExpression).
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn query_reference_is_access(&self, query: &FlowQuery) -> bool {
        match &query.synthetic_props {
            Some(props) => !props.is_empty(),
            None => matches!(
                self.kind_of(query.reference),
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            ),
        }
    }

    /// tsc isMatchingReference(reference.expression, target) for an
    /// access-shaped reference, synthetic-aware (the chain minus its
    /// outermost name is the receiver).
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn query_reference_receiver_matches(
        &mut self,
        query: &FlowQuery,
        target: NodeId,
    ) -> CheckResult2<bool> {
        match &query.synthetic_props {
            Some(props) => {
                let props = props.clone();
                let receiver = &props[..props.len().saturating_sub(1)];
                self.is_matching_synthetic_chain(query.reference, receiver, target)
            }
            None => match self.access_expression_of(query.reference) {
                Some(receiver) => self.is_matching_reference(receiver, target),
                None => Ok(false),
            },
        }
    }

    /// tsc optionalChainContainsReference(expr, reference): UNLIKE
    /// every other narrowing site, the reference is isMatching-
    /// Reference's TARGET here (69557 walks the chain on the source
    /// side) — so the synthetic arm uses the target-oriented matcher
    /// (no assignment/comma unwrap on the real chain parts; those
    /// arms are target-side only). Plain queries keep the vetted node
    /// helper.
    /// tsrs-native: dispatch over FlowQuery::synthetic_props.
    pub(crate) fn optional_chain_contains_query_reference(
        &mut self,
        source: NodeId,
        query: &FlowQuery,
    ) -> CheckResult2<bool> {
        let Some(props) = query.synthetic_props.clone() else {
            return self.optional_chain_contains_reference(source, query.reference);
        };
        let mut source = source;
        loop {
            let file_source = self.binder.source_of_node(source);
            if !node_util::is_optional_chain(file_source, source) {
                return Ok(false);
            }
            let next = match self.data_of(source) {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::CallExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next else {
                return Ok(false);
            };
            source = next;
            if self.real_node_matches_synthetic_chain(source, query.reference, &props)? {
                return Ok(true);
            }
        }
    }

    /// tsc isMatchingReference (69448) with source = a REAL node and
    /// target = the synthetic chain: the target-side unwrap arms
    /// (paren/nonnull/assignment/comma) are no-ops on the factory
    /// access, so only the source-side arms run — paren/nonnull
    /// unwrapping and the access name-and-receiver recursion. The
    /// identifier/this/super source arms compare against an access-
    /// shaped target and are false while chain names remain.
    /// tsrs-native: target-oriented arm of the same tsc span.
    fn real_node_matches_synthetic_chain(
        &mut self,
        source: NodeId,
        base: NodeId,
        props: &[String],
    ) -> CheckResult2<bool> {
        let Some((outer, receiver_props)) = props.split_last() else {
            return self.is_matching_reference(source, base);
        };
        match self.kind_of(source) {
            SyntaxKind::ParenthesizedExpression | SyntaxKind::NonNullExpression => {
                let inner = match self.data_of(source) {
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    NodeData::NonNullExpression(data) => data.expression,
                    _ => None,
                };
                match inner {
                    Some(inner) => self.real_node_matches_synthetic_chain(inner, base, props),
                    None => Ok(false),
                }
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                let Some(source_name) = self.get_accessed_property_name(source)? else {
                    return Ok(false);
                };
                if &source_name != outer {
                    return Ok(false);
                }
                let receiver = match self.data_of(source) {
                    NodeData::PropertyAccessExpression(data) => data.expression,
                    NodeData::ElementAccessExpression(data) => data.expression,
                    _ => None,
                };
                match receiver {
                    Some(receiver) => {
                        self.real_node_matches_synthetic_chain(receiver, base, receiver_props)
                    }
                    None => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }
}

/// The 6.1 PRELUDE UNIT (m5-flow-steps.md: isMatchingReference +
/// getAccessedPropertyName move up from the old 6.5 slot): ported
/// complete and DIRECT per checker-key §4.6, consumed from 6.2 on by
/// the assignment/array-mutation arms (the 6.4 narrowers join later).
impl<'a> CheckerState<'a> {
    /// tsc-port: isMatchingReference @6.0.3
    /// tsc-hash: bfcd27bb28aba7f547933de9d45ca18c2e2882c8be48e69afead82e127fca6b7
    /// tsc-span: _tsc.js:69448-69492
    ///
    /// Ported DIRECTLY (checker-key §4.6): symbol-resolved
    /// identifiers, property/element access by accessed-name +
    /// matching receiver (incl. the constant/unassigned-local index
    /// form), this/super/meta-property, comma and assignment
    /// unwrapping. Two unresolved identifiers compare EQUAL (tsc:
    /// unknownSymbol === unknownSymbol) — our get_resolved_symbol
    /// returns None for unknownSymbol, and None == None.
    pub(crate) fn is_matching_reference(
        &mut self,
        source: NodeId,
        target: NodeId,
    ) -> CheckResult2<bool> {
        match self.kind_of(target) {
            SyntaxKind::ParenthesizedExpression | SyntaxKind::NonNullExpression => {
                let inner = match self.data_of(target) {
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    NodeData::NonNullExpression(data) => data.expression,
                    _ => None,
                };
                if let Some(inner) = inner {
                    return self.is_matching_reference(source, inner);
                }
                return Ok(false);
            }
            SyntaxKind::BinaryExpression => {
                if let NodeData::BinaryExpression(data) = self.data_of(target) {
                    let (left, right, operator) = (data.left, data.right, data.operator_token);
                    if let (Some(left), Some(operator)) = (left, operator) {
                        if node_util::is_assignment_operator(self.kind_of(operator))
                            && self.is_matching_reference(source, left)?
                        {
                            return Ok(true);
                        }
                    }
                    if let (Some(right), Some(operator)) = (right, operator) {
                        if self.kind_of(operator) == SyntaxKind::CommaToken
                            && self.is_matching_reference(source, right)?
                        {
                            return Ok(true);
                        }
                    }
                }
                // tsc's target arm RETURNS here (69455): a binary
                // target that matches through neither its assignment
                // left nor its comma right matches nothing — the
                // source switch is not consulted.
                return Ok(false);
            }
            _ => {}
        }
        match self.kind_of(source) {
            SyntaxKind::MetaProperty => {
                if self.kind_of(target) != SyntaxKind::MetaProperty {
                    return Ok(false);
                }
                // keywordToken equality: the arena stores no token
                // node; new.target vs import.meta is derivable
                // (meta_property_is_new).
                if self.meta_property_is_new(source) != self.meta_property_is_new(target) {
                    return Ok(false);
                }
                let (NodeData::MetaProperty(source_data), NodeData::MetaProperty(target_data)) =
                    (self.data_of(source), self.data_of(target))
                else {
                    return Ok(false);
                };
                let (source_name, target_name) = (source_data.name, target_data.name);
                Ok(self.escaped_text_of(source_name) == self.escaped_text_of(target_name))
            }
            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier => {
                if self.is_this_in_type_query(source) {
                    return Ok(self.kind_of(target) == SyntaxKind::ThisKeyword);
                }
                if self.kind_of(target) == SyntaxKind::Identifier {
                    let source_symbol = self.get_resolved_symbol(source)?;
                    let target_symbol = self.get_resolved_symbol(target)?;
                    if source_symbol == target_symbol {
                        return Ok(true);
                    }
                }
                if matches!(
                    self.kind_of(target),
                    SyntaxKind::VariableDeclaration | SyntaxKind::BindingElement
                ) {
                    let Some(source_symbol) = self.get_resolved_symbol(source)? else {
                        return Ok(false);
                    };
                    let exported =
                        self.get_export_symbol_of_value_symbol_if_exported(source_symbol);
                    return Ok(self
                        .get_symbol_of_declaration_opt(target)
                        .is_some_and(|target_symbol| exported == target_symbol));
                }
                Ok(false)
            }
            SyntaxKind::ThisKeyword => Ok(self.kind_of(target) == SyntaxKind::ThisKeyword),
            SyntaxKind::SuperKeyword => Ok(self.kind_of(target) == SyntaxKind::SuperKeyword),
            SyntaxKind::NonNullExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::SatisfiesExpression => {
                let inner = match self.data_of(source) {
                    NodeData::NonNullExpression(data) => data.expression,
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    NodeData::SatisfiesExpression(data) => data.expression,
                    _ => None,
                };
                match inner {
                    Some(inner) => self.is_matching_reference(inner, target),
                    None => Ok(false),
                }
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                if let Some(source_property_name) = self.get_accessed_property_name(source)? {
                    let target_is_access = matches!(
                        self.kind_of(target),
                        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                    );
                    let target_property_name = if target_is_access {
                        self.get_accessed_property_name(target)?
                    } else {
                        None
                    };
                    if let Some(target_property_name) = target_property_name {
                        if target_property_name != source_property_name {
                            return Ok(false);
                        }
                        let (Some(source_expression), Some(target_expression)) = (
                            self.access_expression_of(source),
                            self.access_expression_of(target),
                        ) else {
                            return Ok(false);
                        };
                        return self.is_matching_reference(source_expression, target_expression);
                    }
                }
                if self.kind_of(source) == SyntaxKind::ElementAccessExpression
                    && self.kind_of(target) == SyntaxKind::ElementAccessExpression
                {
                    let (source_argument, target_argument) = (
                        self.element_access_argument_of(source),
                        self.element_access_argument_of(target),
                    );
                    if let (Some(source_argument), Some(target_argument)) =
                        (source_argument, target_argument)
                    {
                        if self.kind_of(source_argument) == SyntaxKind::Identifier
                            && self.kind_of(target_argument) == SyntaxKind::Identifier
                        {
                            let source_symbol = self.get_resolved_symbol(source_argument)?;
                            let target_symbol = self.get_resolved_symbol(target_argument)?;
                            if let Some(symbol) =
                                source_symbol.filter(|_| source_symbol == target_symbol)
                            {
                                if self.is_constant_variable(symbol)
                                    || (self.is_parameter_or_mutable_local_variable(symbol)
                                        && !self.is_symbol_assigned(symbol)?)
                                {
                                    let (Some(source_expression), Some(target_expression)) = (
                                        self.access_expression_of(source),
                                        self.access_expression_of(target),
                                    ) else {
                                        return Ok(false);
                                    };
                                    return self.is_matching_reference(
                                        source_expression,
                                        target_expression,
                                    );
                                }
                            }
                        }
                    }
                }
                Ok(false)
            }
            SyntaxKind::QualifiedName => {
                if !matches!(
                    self.kind_of(target),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                ) {
                    return Ok(false);
                }
                let NodeData::QualifiedName(data) = self.data_of(source) else {
                    return Ok(false);
                };
                let (left, right) = (data.left, data.right);
                let (Some(left), Some(right)) = (left, right) else {
                    return Ok(false);
                };
                let right_text = self.escaped_text_of(Some(right)).map(str::to_owned);
                let accessed = self.get_accessed_property_name(target)?;
                if right_text.is_none() || right_text != accessed {
                    return Ok(false);
                }
                let Some(target_expression) = self.access_expression_of(target) else {
                    return Ok(false);
                };
                self.is_matching_reference(left, target_expression)
            }
            SyntaxKind::BinaryExpression => {
                let NodeData::BinaryExpression(data) = self.data_of(source) else {
                    return Ok(false);
                };
                let (right, operator) = (data.right, data.operator_token);
                let (Some(right), Some(operator)) = (right, operator) else {
                    return Ok(false);
                };
                if self.kind_of(operator) != SyntaxKind::CommaToken {
                    return Ok(false);
                }
                self.is_matching_reference(right, target)
            }
            _ => Ok(false),
        }
    }

    /// tsc node.symbol without the merged hop (getSymbolOfDeclaration).
    /// tsrs-native: binder node_symbol read behind the declaration
    /// merge (isMatchingReference compares against the export-mapped
    /// merged symbol, so the declaration side merges too).
    pub(crate) fn get_symbol_of_declaration_opt(&self, declaration: NodeId) -> Option<SymbolId> {
        self.binder
            .node_symbol(declaration)
            .map(|symbol| self.get_merged_symbol(symbol))
    }

    /// The `.expression` receiver of an access expression.
    /// tsrs-native: NodeData accessor.
    fn access_expression_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        }
    }

    /// The `.argumentExpression` of an element access.
    /// tsrs-native: NodeData accessor.
    fn element_access_argument_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::ElementAccessExpression(data) => data.argument_expression,
            _ => None,
        }
    }

    /// The escaped text of an identifier-like name node.
    /// tsrs-native: NodeData accessor.
    pub(crate) fn escaped_text_of(&self, node: Option<NodeId>) -> Option<&str> {
        let node = node?;
        match self.data_of(node) {
            NodeData::Identifier(data) => Some(data.escaped_text.as_str()),
            NodeData::PrivateIdentifier(data) => Some(data.escaped_text.as_str()),
            _ => None,
        }
    }

    /// tsc-port: getAccessedPropertyName @6.0.3
    /// tsc-hash: 775e4e1ff558a01ca3e9378a6db2bd6b7a93ff278365c4389443aaacb27dd0b6
    /// tsc-span: _tsc.js:69493-69508
    pub(crate) fn get_accessed_property_name(
        &mut self,
        access: NodeId,
    ) -> CheckResult2<Option<String>> {
        match self.kind_of(access) {
            SyntaxKind::PropertyAccessExpression => {
                let NodeData::PropertyAccessExpression(data) = self.data_of(access) else {
                    return Ok(None);
                };
                Ok(self.escaped_text_of(data.name).map(str::to_owned))
            }
            SyntaxKind::ElementAccessExpression => {
                self.try_get_element_access_expression_name(access)
            }
            SyntaxKind::BindingElement => {
                let name = self.get_destructuring_property_name(access)?;
                Ok(name.map(|name| tsrs2_syntax::escape_leading_underscores(&name)))
            }
            SyntaxKind::Parameter => {
                let Some(parent) = self.parent_of(access) else {
                    return Ok(None);
                };
                let parameters = match self.data_of(parent) {
                    NodeData::FunctionDeclaration(data) => data.parameters,
                    NodeData::FunctionExpression(data) => data.parameters,
                    NodeData::ArrowFunction(data) => data.parameters,
                    NodeData::MethodDeclaration(data) => data.parameters,
                    NodeData::Constructor(data) => data.parameters,
                    NodeData::GetAccessor(data) => data.parameters,
                    NodeData::SetAccessor(data) => data.parameters,
                    _ => None,
                };
                let Some(parameters) = parameters else {
                    return Ok(None);
                };
                let index = self
                    .binder
                    .node_array(parameters)
                    .nodes
                    .iter()
                    .position(|&parameter| parameter == access);
                Ok(index.map(|index| index.to_string()))
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: tryGetNameFromType @6.0.3
    /// tsc-hash: 7a5ee7d20b95577c6e5e15f5c17f14c80d0ed861a6e6f060938a60e33cd9f671
    /// tsc-span: _tsc.js:69509-69511
    pub(crate) fn try_get_name_from_type(&self, ty: TypeId) -> Option<String> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
            if let TypeData::UniqueESSymbol { escaped_name } = &self.tables.type_of(ty).data {
                return Some(escaped_name.clone());
            }
            return None;
        }
        if flags.intersects(TypeFlags::STRING_OR_NUMBER_LITERAL) {
            // tsc: escapeLeadingUnderscores("" + type.value).
            if let TypeData::Literal { value } = &self.tables.type_of(ty).data {
                let text = match value {
                    tsrs2_types::LiteralValue::String(text) => text.clone(),
                    tsrs2_types::LiteralValue::Number(number) => {
                        tsrs2_types::tables::js_number_to_string(*number)
                    }
                    tsrs2_types::LiteralValue::BigInt(_) => return None,
                };
                return Some(tsrs2_syntax::escape_leading_underscores(&text));
            }
            return None;
        }
        None
    }

    /// tsc-port: tryGetElementAccessExpressionName @6.0.3
    /// tsc-hash: 1daecf8e70c80e9850c15614e61823ff533f6bacc75f4bfb00b352888f68c60a
    /// tsc-span: _tsc.js:69512-69514
    fn try_get_element_access_expression_name(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<String>> {
        let Some(argument) = self.element_access_argument_of(node) else {
            return Ok(None);
        };
        let source = self.binder.source_of_node(node);
        if node_util::is_string_or_numeric_literal_like(source, argument) {
            let text = match self.data_of(argument) {
                NodeData::StringLiteral(data) => data.text.clone(),
                NodeData::NumericLiteral(data) => data.text.clone(),
                NodeData::NoSubstitutionTemplateLiteral(data) => data.text.clone(),
                _ => return Ok(None),
            };
            return Ok(Some(tsrs2_syntax::escape_leading_underscores(&text)));
        }
        if node_util::is_entity_name_expression(source, argument) {
            return self.try_get_name_from_entity_name_expression(argument);
        }
        Ok(None)
    }

    /// tsc-port: tryGetNameFromEntityNameExpression @6.0.3
    /// tsc-hash: 5f35d70153b842f929c2e08c60f78b22d0ac56fff1a2d6be86b4844e0db96b88
    /// tsc-span: _tsc.js:69515-69543
    fn try_get_name_from_entity_name_expression(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<String>> {
        let Some(symbol) =
            self.resolve_entity_name(node, SymbolFlags::VALUE, /*ignore_errors*/ true, None)?
        else {
            return Ok(None);
        };
        let is_enum_member = self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ENUM_MEMBER);
        if !(self.is_constant_variable(symbol) || is_enum_member) {
            return Ok(None);
        }
        let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
            return Ok(None);
        };
        if let Some(type_node_type) = self.try_get_type_from_effective_type_node(declaration)? {
            if let Some(name) = self.try_get_name_from_type(type_node_type) {
                return Ok(Some(name));
            }
        }
        let has_only_expression_initializer = matches!(
            self.kind_of(declaration),
            SyntaxKind::VariableDeclaration
                | SyntaxKind::Parameter
                | SyntaxKind::BindingElement
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::EnumMember
        );
        if has_only_expression_initializer
            && self.is_block_scoped_name_declared_before_use(declaration, node)?
        {
            let initializer = match self.data_of(declaration) {
                NodeData::VariableDeclaration(data) => data.initializer,
                NodeData::Parameter(data) => data.initializer,
                NodeData::BindingElement(data) => data.initializer,
                NodeData::PropertyDeclaration(data) => data.initializer,
                NodeData::PropertyAssignment(data) => data.initializer,
                NodeData::EnumMember(data) => data.initializer,
                _ => None,
            };
            if let Some(initializer) = initializer {
                let is_pattern_parent = self.parent_of(declaration).is_some_and(|parent| {
                    matches!(
                        self.kind_of(parent),
                        SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                    )
                });
                let initializer_type = if is_pattern_parent {
                    self.get_type_for_binding_element_of_flow(declaration)?
                } else {
                    Some(self.get_type_of_expression(initializer)?)
                };
                return Ok(initializer_type.and_then(|ty| self.try_get_name_from_type(ty)));
            }
            if self.kind_of(declaration) == SyntaxKind::EnumMember {
                let NodeData::EnumMember(data) = self.data_of(declaration) else {
                    return Ok(None);
                };
                let Some(name) = data.name else {
                    return Ok(None);
                };
                return Ok(Some(self.get_text_of_property_name(name)?));
            }
        }
        Ok(None)
    }

    /// tsc getTypeForBindingElement (55942) — the checkMode selection
    /// over getTypeForBindingElementParent, as consumed by
    /// tryGetNameFromEntityNameExpression's binding-pattern arm.
    /// tsrs-native: thin dispatch over get_type_for_binding_element_parent
    /// + get_binding_element_type_from_parent_type (both ported).
    fn get_type_for_binding_element_of_flow(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let check_mode = match self.data_of(declaration) {
            NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some() => {
                CheckMode::REST_BINDING_ELEMENT
            }
            _ => CheckMode::NORMAL,
        };
        let Some(grandparent) = self
            .parent_of(declaration)
            .and_then(|parent| self.parent_of(parent))
        else {
            return Ok(None);
        };
        let Some(parent_type) =
            self.get_type_for_binding_element_parent(grandparent, check_mode)?
        else {
            return Ok(None);
        };
        Ok(Some(self.get_binding_element_type_from_parent_type(
            declaration,
            parent_type,
            /*no_tuple_bounds_check*/ false,
        )?))
    }

    /// tsc-port: getDestructuringPropertyName @6.0.3
    /// tsc-hash: 1dbd2a17e292c118cde00841a6132ad158c447ad16ad6dc5e1a71ca8ec6ea2a0
    /// tsc-span: _tsc.js:55928-55937
    fn get_destructuring_property_name(&mut self, node: NodeId) -> CheckResult2<Option<String>> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        if self.kind_of(node) == SyntaxKind::BindingElement
            && self.kind_of(parent) == SyntaxKind::ObjectBindingPattern
        {
            let NodeData::BindingElement(data) = self.data_of(node) else {
                return Ok(None);
            };
            let Some(name) = data.property_name.or(data.name) else {
                return Ok(None);
            };
            return self.get_literal_property_name_text(name);
        }
        if matches!(
            self.kind_of(node),
            SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment
        ) {
            let name = match self.data_of(node) {
                NodeData::PropertyAssignment(data) => data.name,
                NodeData::ShorthandPropertyAssignment(data) => data.name,
                _ => None,
            };
            let Some(name) = name else {
                return Ok(None);
            };
            return self.get_literal_property_name_text(name);
        }
        let elements = match self.data_of(parent) {
            NodeData::ArrayBindingPattern(data) => data.elements,
            NodeData::ObjectBindingPattern(data) => data.elements,
            // The assignment-destructuring form: an array-literal
            // element's name is its position (`parent.elements`
            // covers both pattern and literal parents in tsc; the
            // 6.4b getFlowTypeOfDestructuring un-stub is the first
            // caller to reach the literal shape).
            NodeData::ArrayLiteralExpression(data) => data.elements,
            _ => None,
        };
        let Some(elements) = elements else {
            return Ok(None);
        };
        let index = self
            .binder
            .node_array(elements)
            .nodes
            .iter()
            .position(|&element| element == node);
        Ok(index.map(|index| index.to_string()))
    }

    /// tsc-port: getLiteralPropertyNameText @6.0.3
    /// tsc-hash: ab1c220ebc53b052cfc7338dc7285089cb02d0be1612cc32eccff29941db8669
    /// tsc-span: _tsc.js:55938-55941
    fn get_literal_property_name_text(&mut self, name: NodeId) -> CheckResult2<Option<String>> {
        let ty = self.get_literal_type_from_property_name(name)?;
        if !self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL)
        {
            return Ok(None);
        }
        if let TypeData::Literal { value } = &self.tables.type_of(ty).data {
            return Ok(match value {
                tsrs2_types::LiteralValue::String(text) => Some(text.clone()),
                tsrs2_types::LiteralValue::Number(number) => {
                    Some(tsrs2_types::tables::js_number_to_string(*number))
                }
                tsrs2_types::LiteralValue::BigInt(_) => None,
            });
        }
        Ok(None)
    }
}

impl<'a> CheckerState<'a> {
    // ---- the assignment-marking family ----

    /// tsc-port: isSymbolAssignedDefinitely @6.0.3
    /// tsc-hash: 8ef630d8e573b3afa896293dc2d82d43292a9ce5db97be59e2d8e25df20da7ec
    /// tsc-span: _tsc.js:71480-71485
    ///
    /// The second read after isSymbolAssigned is live in tsc: the
    /// marking pass may SET lastAssignmentPos as a side effect.
    pub(crate) fn is_symbol_assigned_definitely(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        if let Some(pos) = self.links.symbol(symbol).last_assignment_pos {
            return Ok(pos < 0);
        }
        Ok(self.is_symbol_assigned(symbol)?
            && self
                .links
                .symbol(symbol)
                .last_assignment_pos
                .is_some_and(|pos| pos < 0))
    }

    /// tsc-port: isSymbolAssigned @6.0.3
    /// tsc-hash: 21f5b82c9471f79e17d61052683107b8e44fd59d90c11faf9ddf62da63111bd4
    /// tsc-span: _tsc.js:71486-71492
    pub(crate) fn is_symbol_assigned(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        Ok(!self.is_past_last_assignment(symbol, None)?)
    }

    /// tsc-port: isPastLastAssignment @6.0.3
    /// tsc-hash: 4575c28648468629a2d9c45b111040618cf185a3e7a5c6b4308e0807c1839c1d
    /// tsc-span: _tsc.js:71493-71506
    ///
    /// tsc's `!symbol.lastAssignmentPos` treats BOTH the unmarked
    /// state and position 0 as "no assignment" (JS falsiness) — kept
    /// faithfully. tsc cannot fail inside the marking pass; ours can
    /// unwind (getResolvedSymbol), so the AssignmentsMarked latch
    /// REVERTS on Err — a re-query re-marks instead of trusting a
    /// half-marked container (the unwind invariant).
    pub(crate) fn is_past_last_assignment(
        &mut self,
        symbol: SymbolId,
        location: Option<NodeId>,
    ) -> CheckResult2<bool> {
        let Some(value_declaration) = self.binder.symbol(symbol).value_declaration else {
            return Ok(false);
        };
        let Some(parent) = self.find_ancestor(Some(value_declaration), |state, n| {
            if state.is_function_or_source_file(n) {
                crate::expr::Ancestor::Yes
            } else {
                crate::expr::Ancestor::No
            }
        }) else {
            return Ok(false);
        };
        if !self
            .links
            .node(parent)
            .check_flags
            .intersects(NodeCheckFlags::ASSIGNMENTS_MARKED)
        {
            self.links.or_node_check_flags(
                self.speculation_depth,
                parent,
                NodeCheckFlags::ASSIGNMENTS_MARKED,
            );
            if !self.has_parent_with_assignments_marked(parent) {
                let marked = self.mark_node_assignments(parent);
                if marked.is_err() {
                    self.links.clear_node_check_flags(
                        self.speculation_depth,
                        parent,
                        NodeCheckFlags::ASSIGNMENTS_MARKED,
                    );
                }
                marked?;
            }
        }
        let pos = self.links.symbol(symbol).last_assignment_pos;
        Ok(match pos {
            None | Some(0) => true,
            Some(pos) => location.is_some_and(|location| {
                (pos.unsigned_abs() as usize) < self.pos_of(location) as usize
            }),
        })
    }

    /// tsc-port: isSomeSymbolAssigned @6.0.3
    /// tsc-hash: eaf58fff03c42260e23b4ddfa301747a15b3f2301b6af193fc0b9935b256aa65
    /// tsc-span: _tsc.js:71507-71510
    pub(crate) fn is_some_symbol_assigned(
        &mut self,
        root_declaration: NodeId,
    ) -> CheckResult2<bool> {
        debug_assert!(matches!(
            self.kind_of(root_declaration),
            SyntaxKind::VariableDeclaration | SyntaxKind::Parameter
        ));
        let name = match self.data_of(root_declaration) {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::Parameter(data) => data.name,
            _ => None,
        };
        match name {
            Some(name) => self.is_some_symbol_assigned_worker(name),
            None => Ok(false),
        }
    }

    /// tsc-port: isSomeSymbolAssignedWorker @6.0.3
    /// tsc-hash: fef48cbb2f53792c2b75f29bef411642cdff57c654c9de70aba38154506940ae
    /// tsc-span: _tsc.js:71511-71516
    fn is_some_symbol_assigned_worker(&mut self, node: NodeId) -> CheckResult2<bool> {
        if self.kind_of(node) == SyntaxKind::Identifier {
            let Some(parent) = self.parent_of(node) else {
                return Ok(false);
            };
            let Some(symbol) = self.get_symbol_of_declaration_opt(parent) else {
                return Ok(false);
            };
            return self.is_symbol_assigned(symbol);
        }
        let elements = match self.data_of(node) {
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        let Some(elements) = elements else {
            return Ok(false);
        };
        let elements: Vec<NodeId> = self.binder.node_array(elements).nodes.clone();
        for element in elements {
            if self.kind_of(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let name = match self.data_of(element) {
                NodeData::BindingElement(data) => data.name,
                _ => None,
            };
            if let Some(name) = name {
                if self.is_some_symbol_assigned_worker(name)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: hasParentWithAssignmentsMarked @6.0.3
    /// tsc-hash: 021a92e7834f4fb14b5447f995179587d2b69dff7f44c5175f6354550c9a6fda
    /// tsc-span: _tsc.js:71517-71519
    fn has_parent_with_assignments_marked(&mut self, node: NodeId) -> bool {
        self.find_ancestor(self.parent_of(node), |state, n| {
            if state.is_function_or_source_file(n)
                && state
                    .links
                    .node(n)
                    .check_flags
                    .intersects(NodeCheckFlags::ASSIGNMENTS_MARKED)
            {
                crate::expr::Ancestor::Yes
            } else {
                crate::expr::Ancestor::No
            }
        })
        .is_some()
    }

    /// tsc-port: isFunctionOrSourceFile @6.0.3
    /// tsc-hash: 15f0fe3d4c5a37c9c1e2080f332ef40d7f2faaaccae838b5265289e598febc1a
    /// tsc-span: _tsc.js:71520-71522
    fn is_function_or_source_file(&self, node: NodeId) -> bool {
        let kind = self.kind_of(node);
        node_util::is_function_like_declaration_kind(kind) || kind == SyntaxKind::SourceFile
    }

    /// tsc-port: markNodeAssignments @6.0.3
    /// tsc-hash: 1affd0e5d028cef40a79b696dd8b5acc4281692ff5c8c355d6601667e1143ed1
    /// tsc-span: _tsc.js:71523-71569
    ///
    /// tsc recurses via forEachChild; deep expression chains exist in
    /// the corpus (M2 ground rule: walkers are ITERATIVE), so this is
    /// an explicit pre-order stack with children pushed reversed —
    /// write order stays document order, and the LAST assignment's
    /// write wins exactly as in tsc.
    fn mark_node_assignments(&mut self, root: NodeId) -> CheckResult2<()> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match self.kind_of(node) {
                SyntaxKind::Identifier => {
                    let assignment_target = self.get_assignment_target_kind(node);
                    if assignment_target != crate::expr::AssignmentKind::None {
                        let Some(symbol) = self.get_resolved_symbol(node)? else {
                            continue;
                        };
                        let previous = self.links.symbol(symbol).last_assignment_pos;
                        let has_definite_assignment = assignment_target
                            == crate::expr::AssignmentKind::Definite
                            || previous.is_some_and(|pos| pos < 0);
                        if self.is_parameter_or_mutable_local_variable(symbol) {
                            if previous.is_none_or(|pos| pos.abs() != i64::MAX) {
                                let referencing_function =
                                    self.find_ancestor(Some(node), |state, n| {
                                        if state.is_function_or_source_file(n) {
                                            crate::expr::Ancestor::Yes
                                        } else {
                                            crate::expr::Ancestor::No
                                        }
                                    });
                                let declaring_function =
                                    self.binder.symbol(symbol).value_declaration.and_then(
                                        |declaration| {
                                            self.find_ancestor(Some(declaration), |state, n| {
                                                if state.is_function_or_source_file(n) {
                                                    crate::expr::Ancestor::Yes
                                                } else {
                                                    crate::expr::Ancestor::No
                                                }
                                            })
                                        },
                                    );
                                let pos = if referencing_function == declaring_function {
                                    let declaration = self
                                        .binder
                                        .symbol(symbol)
                                        .value_declaration
                                        .expect("parameter-or-mutable-local has a declaration");
                                    self.extend_assignment_position(node, declaration)
                                } else {
                                    i64::MAX
                                };
                                self.links.set_symbol_last_assignment_pos(
                                    self.speculation_depth,
                                    symbol,
                                    Some(pos),
                                );
                            }
                            if has_definite_assignment {
                                let current = self.links.symbol(symbol).last_assignment_pos;
                                if let Some(pos) = current {
                                    if pos > 0 {
                                        self.links.set_symbol_last_assignment_pos(
                                            self.speculation_depth,
                                            symbol,
                                            Some(-pos),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                SyntaxKind::ExportSpecifier => {
                    self.mark_export_specifier_assignment(node)?;
                    continue;
                }
                SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::EnumDeclaration => continue,
                kind if self.is_type_node_kind(kind) => continue,
                _ => {}
            }
            let source = self.binder.source_of_node(node);
            let mut children: Vec<NodeId> = Vec::new();
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            stack.extend(children.into_iter().rev());
        }
        Ok(())
    }

    /// The ExportSpecifier arm of markNodeAssignments (71549-71565):
    /// a mutable local re-exported by value gets the MAX sentinel
    /// ("assigned somewhere unknowable").
    /// tsrs-native: extracted arm of mark_node_assignments (same span).
    fn mark_export_specifier_assignment(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::ExportSpecifier(data) = self.data_of(node) else {
            return Ok(());
        };
        if data.is_type_only {
            return Ok(());
        }
        let Some(name) = data.property_name.or(data.name) else {
            return Ok(());
        };
        let Some(export_declaration) = self
            .parent_of(node)
            .and_then(|named_exports| self.parent_of(named_exports))
        else {
            return Ok(());
        };
        let NodeData::ExportDeclaration(export_data) = self.data_of(export_declaration) else {
            return Ok(());
        };
        if export_data.is_type_only || export_data.module_specifier.is_some() {
            return Ok(());
        }
        if self.kind_of(name) == SyntaxKind::StringLiteral {
            return Ok(());
        }
        let Some(symbol) = self.resolve_entity_name_ex(
            name,
            SymbolFlags::VALUE,
            /*ignore_errors*/ true,
            None,
            /*dont_resolve_alias*/ true,
        )?
        else {
            return Ok(());
        };
        if self.is_parameter_or_mutable_local_variable(symbol) {
            let sign = if self
                .links
                .symbol(symbol)
                .last_assignment_pos
                .is_some_and(|pos| pos < 0)
            {
                -1
            } else {
                1
            };
            self.links.set_symbol_last_assignment_pos(
                self.speculation_depth,
                symbol,
                Some(sign * i64::MAX),
            );
        }
        Ok(())
    }

    /// tsc-port: extendAssignmentPosition @6.0.3
    /// tsc-hash: 595fb9c3fc639921a8e3689162a0d996b4245e5e17c5b9cd61257441ce05b2b8
    /// tsc-span: _tsc.js:71570-71591
    ///
    /// The assignment position extends to the END of any enclosing
    /// statement/class that starts after the declaration — a
    /// conservative "the assignment may re-execute anywhere in here"
    /// widening.
    fn extend_assignment_position(&self, node: NodeId, declaration: NodeId) -> i64 {
        let declaration_pos = self.pos_of(declaration);
        let mut pos = self.pos_of(node) as i64;
        let mut current = Some(node);
        while let Some(node) = current {
            if self.pos_of(node) <= declaration_pos {
                break;
            }
            match self.kind_of(node) {
                SyntaxKind::VariableStatement
                | SyntaxKind::ExpressionStatement
                | SyntaxKind::IfStatement
                | SyntaxKind::DoStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::WithStatement
                | SyntaxKind::SwitchStatement
                | SyntaxKind::TryStatement
                | SyntaxKind::ClassDeclaration => pos = self.end_of(node) as i64,
                _ => {}
            }
            current = self.parent_of(node);
        }
        pos
    }

    // (isConstantVariable lives in evaluate.rs and
    // isParameterOrMutableLocalVariable in functions.rs — both landed
    // during M4; the 6.1 prelude consumes them as-is.)

    // ---- getNarrowedTypeOfSymbol (checkIdentifier's entry type) ----

    /// tsc-port: getNarrowedTypeOfSymbol @6.0.3
    /// tsc-hash: 22f3776b5ae1c8cd1ecef7799b03eb16ccc169f3bfe6b062b5bad3d2bce43ce9
    /// tsc-span: _tsc.js:72001-72062
    ///
    /// Arm 1 (destructured discriminated-union narrowing over the
    /// pattern) is LIVE: it re-resolves the binding element from the
    /// parent's constraint through getFlowTypeOfReference at the
    /// reference's flow node. Arm 2 (context-sensitive rest-parameter
    /// slices) is LIVE for concrete rest types: tsc's only
    /// M6-dependent read is `getInferenceContext(func)?.nonFixingMapper`
    /// (72044) inside the restType computation, and no inference
    /// context can exist before M6 (instantiateType(T, undefined) = T)
    /// — a rest type that could still contain type variables is the
    /// one shape where the mapper would matter, and stays a named
    /// Unsupported.
    pub(crate) fn get_narrowed_type_of_symbol(
        &mut self,
        symbol: SymbolId,
        location: NodeId,
    ) -> CheckResult2<TypeId> {
        let ty = self.get_type_of_symbol(symbol)?;
        let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
            return Ok(ty);
        };
        if self.kind_of(declaration) == SyntaxKind::BindingElement {
            if let Some(narrowed) = self.narrowed_binding_element_type(declaration, location)? {
                return Ok(narrowed);
            }
        }
        if self.kind_of(declaration) == SyntaxKind::Parameter {
            let NodeData::Parameter(data) = self.data_of(declaration) else {
                return Ok(ty);
            };
            if data.r#type.is_none()
                && data.initializer.is_none()
                && data.dot_dot_dot_token.is_none()
            {
                let Some(func) = self.parent_of(declaration) else {
                    return Ok(ty);
                };
                let parameters: Vec<NodeId> = match self.data_of(func) {
                    NodeData::FunctionExpression(f) => f.parameters,
                    NodeData::ArrowFunction(f) => f.parameters,
                    NodeData::FunctionDeclaration(f) => f.parameters,
                    NodeData::MethodDeclaration(f) => f.parameters,
                    _ => None,
                }
                .map(|list| self.binder.node_array(list).nodes.clone())
                .unwrap_or_default();
                if parameters.len() >= 2
                    && self.is_context_sensitive_function_or_object_literal_method(func)?
                {
                    let contextual_signature = self.get_contextual_signature(func)?;
                    if let Some(contextual_signature) = contextual_signature {
                        let (rest_parameter, has_rest) = {
                            let signature = self.signature_of(contextual_signature);
                            (
                                (signature.parameters.len() == 1).then(|| signature.parameters[0]),
                                signature
                                    .flags
                                    .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER),
                            )
                        };
                        if let (Some(rest_parameter), true) = (rest_parameter, has_rest) {
                            let rest_symbol_type = self.get_type_of_symbol(rest_parameter)?;
                            let rest_type = self.get_reduced_apparent_type(rest_symbol_type)?;
                            if self.could_contain_type_variables(rest_type) {
                                // 72044: tsc instantiates the rest type
                                // under getInferenceContext(func)
                                // ?.nonFixingMapper before the tuple
                                // gate — the M6 slice.
                                return Err(Unsupported::new(
                                    "dependent-parameter narrowing over a generic rest type (getInferenceContext nonFixingMapper, M6)",
                                ));
                            }
                            let is_union_of_tuples =
                                self.tables.flags_of(rest_type).intersects(TypeFlags::UNION)
                                    && self.every_type(rest_type, |state, t| {
                                        state.tables.is_tuple_type(t)
                                    });
                            // Gate order is tsc's: the isSomeSymbolAssigned
                            // sweep (a marking pass with side effects) runs
                            // only behind the union-of-tuples test.
                            if is_union_of_tuples && !self.some_parameter_assigned(&parameters)? {
                                let location_flow = self.flow_node_of(location);
                                let narrowed_type = self.get_flow_type_of_reference_with_flow(
                                    func,
                                    rest_type,
                                    rest_type,
                                    None,
                                    location_flow,
                                )?;
                                let has_this_parameter = parameters
                                    .first()
                                    .and_then(|&first| match self.data_of(first) {
                                        NodeData::Parameter(parameter) => parameter.name,
                                        _ => None,
                                    })
                                    .is_some_and(|name| self.is_this_identifier(name));
                                let Some(position) =
                                    parameters.iter().position(|&p| p == declaration)
                                else {
                                    return Ok(ty);
                                };
                                let index =
                                    position as i64 - if has_this_parameter { 1 } else { 0 };
                                let index_type = self.tables.get_number_literal_type(index as f64);
                                return self.get_indexed_access_type(
                                    narrowed_type,
                                    index_type,
                                    tsrs2_types::AccessFlags::NONE,
                                    None,
                                    None,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(ty)
    }

    /// The `some(func.parameters, isSomeSymbolAssigned)` composition
    /// (getNarrowedTypeOfSymbol 72045) — the fallible-some over the
    /// parameter list.
    /// tsrs-native: fallible-some instance.
    fn some_parameter_assigned(&mut self, parameters: &[NodeId]) -> CheckResult2<bool> {
        for &parameter in parameters {
            if self.is_some_symbol_assigned(parameter)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Arm 1 of getNarrowedTypeOfSymbol (72006-72039): a
    /// non-rest, non-initialized binding element in a ≥2-element
    /// pattern under a const/parameter root, whose parent's constraint
    /// is a union, re-resolves through the flow at `location`.
    /// tsrs-native: extracted arm of get_narrowed_type_of_symbol
    /// (same span; the InCheckIdentifier re-entrance latch clears on
    /// unwind).
    fn narrowed_binding_element_type(
        &mut self,
        declaration: NodeId,
        location: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let NodeData::BindingElement(data) = self.data_of(declaration) else {
            return Ok(None);
        };
        if data.initializer.is_some() || data.dot_dot_dot_token.is_some() {
            return Ok(None);
        }
        let Some(pattern) = self.parent_of(declaration) else {
            return Ok(None);
        };
        let element_count = match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(p) => p.elements,
            NodeData::ArrayBindingPattern(p) => p.elements,
            _ => None,
        }
        .map(|elements| self.binder.node_array(elements).nodes.len())
        .unwrap_or(0);
        if element_count < 2 {
            return Ok(None);
        }
        let Some(parent) = self.parent_of(pattern) else {
            return Ok(None);
        };
        let source = self.binder.source_of_node(parent);
        let root_declaration = node_util::get_root_declaration(source, parent);
        let root_is_const_variable = self.kind_of(root_declaration)
            == SyntaxKind::VariableDeclaration
            && node_util::get_combined_node_flags(source, root_declaration)
                .intersects(NodeFlags::CONSTANT);
        let root_is_parameter = self.kind_of(root_declaration) == SyntaxKind::Parameter;
        if !root_is_const_variable && !root_is_parameter {
            return Ok(None);
        }
        if self
            .links
            .node(parent)
            .check_flags
            .intersects(NodeCheckFlags::IN_CHECK_IDENTIFIER)
        {
            return Ok(None);
        }
        self.links.or_node_check_flags(
            self.speculation_depth,
            parent,
            NodeCheckFlags::IN_CHECK_IDENTIFIER,
        );
        let parent_type_result =
            self.get_type_for_binding_element_parent(parent, CheckMode::NORMAL);
        // The re-entrance latch clears BEFORE the `?` (unwind
        // invariant: no InCheckIdentifier residue).
        self.links.clear_node_check_flags(
            self.speculation_depth,
            parent,
            NodeCheckFlags::IN_CHECK_IDENTIFIER,
        );
        let Some(parent_type) = parent_type_result? else {
            return Ok(None);
        };
        let parent_type_constraint =
            self.map_type_result(parent_type, |state, t| state.get_base_constraint_or_type(t))?;
        if !self
            .tables
            .flags_of(parent_type_constraint)
            .intersects(TypeFlags::UNION)
        {
            return Ok(None);
        }
        if root_is_parameter && self.is_some_symbol_assigned(root_declaration)? {
            return Ok(None);
        }
        let location_flow = self.flow_node_of(location);
        let narrowed_type = self.get_flow_type_of_reference_with_flow(
            pattern,
            parent_type_constraint,
            parent_type_constraint,
            None,
            location_flow,
        )?;
        if self
            .tables
            .flags_of(narrowed_type)
            .intersects(TypeFlags::NEVER)
        {
            return Ok(Some(self.tables.intrinsics.never));
        }
        Ok(Some(self.get_binding_element_type_from_parent_type(
            declaration,
            narrowed_type,
            /*no_tuple_bounds_check*/ true,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::FlowType;

    #[test]
    fn flow_type_accessors() {
        let ty = tsrs2_types::TypeId(7);
        assert_eq!(FlowType::Type(ty).get_type(), ty);
        assert_eq!(FlowType::Incomplete(ty).get_type(), ty);
        assert!(!FlowType::Type(ty).is_incomplete());
        assert!(FlowType::Incomplete(ty).is_incomplete());
    }
}
