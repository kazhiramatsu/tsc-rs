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
//! Stage state (6.1): the walk skeleton is live and REPLACES
//! `get_flow_type_of_reference_stub`; the arms are inert —
//! - [FLOW 6.2] assignment/array-mutation: an assignment terminates
//!   with the declared type (opaque write; walking PAST it would
//!   resurrect an undefined-bearing initial type at uses the
//!   assignment kills); mutations never affect (no evolving arrays).
//! - [FLOW 6.3] branch/loop joins return the declared type.
//! - [FLOW 6.4] conditions/switch clauses pass through their
//!   antecedent (tsc's arm with narrowType = identity); calls never
//!   affect (no effects signatures yet).
//!
//! With `initialType` still passed as the declared type by every
//! caller (the checkIdentifier initialType ladder activates with the
//! real assignment arm, 6.2), every query resolves to the declared
//! type — the stub swap is observably null, which is this stage's
//! verification.

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
    // [FLOW 6.3] first constructed by the loop-label fixpoint.
    #[allow(dead_code)]
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
    ///
    /// [FLOW 6.3] first consumed by the join/fixpoint arms.
    #[allow(dead_code)]
    pub(crate) fn is_incomplete(self) -> bool {
        matches!(self, FlowType::Incomplete(_))
    }
}

/// The per-query locals of tsc getFlowTypeOfReference (70394): the
/// reference bindings plus `flowDepth` (reset per query — NOT checker
/// state) and this query's window into the shared-flow cache. The
/// loop-label cache `key`/`isKeySet` pair joins at 6.3.
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
}

impl<'a> CheckerState<'a> {
    /// tsc-port: createFlowType @6.0.3
    /// tsc-hash: 6c62e978ce2e99049bfa9f92da21d61691a7fd53aea3085dd19d06425de9526b
    /// tsc-span: _tsc.js:70070-70072
    ///
    /// `never` is replaced by silentNeverType INSIDE incomplete
    /// wrappers — "back-edge unresolved" must stay distinguishable
    /// from a real never.
    ///
    /// [FLOW 6.3] first consumed by the condition/join arms once
    /// incomplete types circulate.
    #[allow(dead_code)]
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
        if self.flow_analysis_disabled {
            return Ok(self.tables.intrinsics.error);
        }
        let Some(flow_node) = flow_node else {
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
        };
        let walk = self.get_type_at_flow_node(&mut query, flow_node);
        // sharedFlowCount = sharedFlowStart — restored BEFORE the `?`
        // so an Unsupported unwind leaves no shared-cache residue (the
        // unwind invariant).
        self.shared_flow.truncate(shared_flow_start);
        let evolved_type = walk?.get_type();
        // [FLOW 6.2] the evolving-array postlude
        // (isEvolvingArrayOperationTarget ? autoArrayType :
        // finalizeEvolvingArrayType) is identity until evolving arrays
        // land — no EvolvingArray type has a producer yet.
        let result_type = evolved_type;
        if result_type == self.tables.intrinsics.unreachable_never {
            return Ok(declared_type);
        }
        if self
            .parent_of(reference)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::NonNullExpression)
            && !self
                .tables
                .flags_of(result_type)
                .intersects(TypeFlags::NEVER)
        {
            // [FLOW 6.4] tsc filters `getTypeWithFacts(resultType,
            // NEUndefinedOrNull).flags & Never ⇒ declaredType` here;
            // the facts filter joins the narrowers' stage — in 6.1 the
            // walk returns the declared type for every query, so the
            // filter's outcome is the identity either way.
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
                ty = if flags.intersects(FlowFlags::BRANCH_LABEL) {
                    self.get_type_at_flow_branch_label(query, flow)?
                } else {
                    self.get_type_at_flow_loop_label(query, flow)?
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
                    let reference_kind = self.kind_of(query.reference);
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
                ty = FlowType::Type(self.convert_auto_to_any(query.declared_type));
            }
            if let Some(shared_node) = shared {
                self.shared_flow.push((shared_node, ty));
            }
            query.flow_depth -= 1;
            return Ok(ty);
        }
    }

    // ---- the arms (6.1: inert stubs; see the module doc) ----

    /// [FLOW 6.2] tsc getTypeAtFlowAssignment (70502) lands next
    /// stage. Inert form: any assignment on the path is an opaque
    /// write restoring the declared type — the FP-safe direction
    /// (walking past an assignment would resurrect an
    /// undefined-bearing initial type at uses the assignment kills).
    /// tsc-deferred: M5 (stage 6.2 — assignment/initial-type analysis)
    fn get_type_at_flow_assignment(
        &mut self,
        query: &mut FlowQuery,
        _flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        Ok(Some(FlowType::Type(query.declared_type)))
    }

    /// [FLOW 6.4] tsc getTypeAtFlowCall (70566) rides with the
    /// narrowers (getEffectsSignature + assertion narrowing). Inert
    /// form: no call affects the reference — tsc's own answer for
    /// calls without an effects signature — so the walk continues to
    /// the antecedent.
    /// tsc-deferred: M5 (stage 6.4 — effects signatures + assertions)
    fn get_type_at_flow_call(
        &mut self,
        _query: &mut FlowQuery,
        _flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        Ok(None)
    }

    /// [FLOW 6.4] tsc getTypeAtFlowCondition (70614): the real arm is
    /// narrowType over the antecedent type; until the narrowers land
    /// this is the arm with narrowType = identity — a pass-through of
    /// the antecedent walk.
    /// tsc-deferred: M5 (stage 6.4 — narrowType dispatch)
    fn get_type_at_flow_condition(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let antecedent = self.flow_antecedent(query.file, flow);
        self.get_type_at_flow_node(query, antecedent)
    }

    /// [FLOW 6.4] tsc getTypeAtSwitchClause (70638) narrows by the
    /// switch discriminant; identity until the narrowers land — a
    /// pass-through of the antecedent walk.
    /// tsc-deferred: M5 (stage 6.4 — switch-clause narrowing)
    fn get_type_at_switch_clause(
        &mut self,
        query: &mut FlowQuery,
        flow: FlowId,
    ) -> CheckResult2<FlowType> {
        let antecedent = self.flow_antecedent(query.file, flow);
        self.get_type_at_flow_node(query, antecedent)
    }

    /// [FLOW 6.3] tsc getTypeAtFlowBranchLabel (70653): the JOIN.
    /// Until it lands, a multi-antecedent branch answers the declared
    /// type (the widest join — never narrower than any real union).
    /// tsc-deferred: M5 (stage 6.3 — branch joins)
    fn get_type_at_flow_branch_label(
        &mut self,
        query: &mut FlowQuery,
        _flow: FlowId,
    ) -> CheckResult2<FlowType> {
        Ok(FlowType::Type(query.declared_type))
    }

    /// [FLOW 6.3] tsc getTypeAtFlowLoopLabel (70694): the fixpoint.
    /// Until it lands, a loop join answers the declared type.
    /// tsc-deferred: M5 (stage 6.3 — the loop fixpoint)
    fn get_type_at_flow_loop_label(
        &mut self,
        query: &mut FlowQuery,
        _flow: FlowId,
    ) -> CheckResult2<FlowType> {
        Ok(FlowType::Type(query.declared_type))
    }

    /// [FLOW 6.2] tsc getTypeAtFlowArrayMutation (70588): only
    /// auto/evolving-array references are affected — neither has a
    /// producer until evolving arrays land, so no mutation affects any
    /// reference and the walk continues to the antecedent (tsc's own
    /// answer for non-evolving references).
    /// tsc-deferred: M5 (stage 6.2 — evolving arrays)
    fn get_type_at_flow_array_mutation(
        &mut self,
        _query: &mut FlowQuery,
        _flow: FlowId,
    ) -> CheckResult2<Option<FlowType>> {
        Ok(None)
    }

    // ---- isMatchingReference — narrowing's identity gate ----
}

/// The 6.1 PRELUDE UNIT (m5-flow-steps.md: isMatchingReference +
/// getAccessedPropertyName move up from the old 6.5 slot): ported
/// complete and DIRECT per checker-key §4.6, first consumed by the
/// 6.2 assignment arm and every 6.4 narrower — the allow comes off
/// with the first consumer.
#[allow(dead_code)]
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
    fn get_symbol_of_declaration_opt(&self, declaration: NodeId) -> Option<SymbolId> {
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
    fn escaped_text_of(&self, node: Option<NodeId>) -> Option<&str> {
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
    fn try_get_name_from_type(&self, ty: TypeId) -> Option<String> {
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
    /// slices) reads `getInferenceContext(func).nonFixingMapper` —
    /// M6-deferred, guarded as a named Unsupported at the point the
    /// arm would fire.
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
                let parameter_count = match self.data_of(func) {
                    NodeData::FunctionExpression(f) => f.parameters,
                    NodeData::ArrowFunction(f) => f.parameters,
                    NodeData::FunctionDeclaration(f) => f.parameters,
                    NodeData::MethodDeclaration(f) => f.parameters,
                    _ => None,
                }
                .map(|parameters| self.binder.node_array(parameters).nodes.len())
                .unwrap_or(0);
                if parameter_count >= 2
                    && self.is_context_sensitive_function_or_object_literal_method(func)?
                {
                    let contextual_signature = self.get_contextual_signature(func)?;
                    if let Some(contextual_signature) = contextual_signature {
                        let signature = self.signature_of(contextual_signature);
                        if signature.parameters.len() == 1
                            && signature
                                .flags
                                .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
                        {
                            // The dependent-parameter narrowing slice
                            // instantiates the rest type under the
                            // inference context's nonFixingMapper —
                            // M6 machinery.
                            return Err(Unsupported::new(
                                "dependent-parameter narrowing (getInferenceContext nonFixingMapper, M6)",
                            ));
                        }
                    }
                }
            }
        }
        Ok(ty)
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
