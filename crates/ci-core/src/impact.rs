use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    hash_graph, ActionGraph, ApplicationNamespaceV1, AuthorityReceiptDigestV1, CanonicalEncode,
    CanonicalError, CanonicalSink, GraphDigestV1, GraphValidationError, ImplementationIdV1,
    NodeRecord, ObjectDigestV1, ValidatedGraph,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ImpactError {
    PriorGraph(GraphValidationError),
    CurrentGraph(GraphValidationError),
    CanonicalEncoding,
}

impl fmt::Display for ImpactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "impact error: {self:?}")
    }
}

impl std::error::Error for ImpactError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ImpactPlan<I> {
    changed_prior: Box<[I]>,
    changed_current: Box<[I]>,
    prior_reach: Box<[I]>,
    current_reach: Box<[I]>,
    impacted: Box<[I]>,
}

impl<I> ImpactPlan<I> {
    pub fn changed_prior(&self) -> &[I] {
        &self.changed_prior
    }

    pub fn changed_current(&self) -> &[I] {
        &self.changed_current
    }

    pub fn prior_reach(&self) -> &[I] {
        &self.prior_reach
    }

    pub fn current_reach(&self) -> &[I] {
        &self.current_reach
    }

    pub fn impacted(&self) -> &[I] {
        &self.impacted
    }
}

/// Compare two already sealed graph values and compute both reverse reaches.
/// The prior side is retained so removal and dependency-edge deletion cannot
/// be hidden by projecting immediately onto the current graph.
pub fn compare_graphs<I, K, S>(
    prior: &ActionGraph<I, K, S>,
    current: &ActionGraph<I, K, S>,
) -> Result<ImpactPlan<I>, ImpactError>
where
    I: Clone + Ord + CanonicalEncode,
    K: Eq,
    S: Eq,
{
    let prior_validated = crate::validate_graph(prior).map_err(ImpactError::PriorGraph)?;
    let current_validated = crate::validate_graph(current).map_err(ImpactError::CurrentGraph)?;
    let prior_nodes = node_map(prior);
    let current_nodes = node_map(current);

    let mut ids = BTreeSet::new();
    ids.extend(prior_nodes.keys().cloned());
    ids.extend(current_nodes.keys().cloned());

    let mut changed_prior = BTreeSet::new();
    let mut changed_current = BTreeSet::new();
    for id in &ids {
        let changed = match (prior_nodes.get(id), current_nodes.get(id)) {
            (Some(prior_node), Some(current_node)) => *prior_node != *current_node,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if changed {
            if prior_nodes.contains_key(id) {
                changed_prior.insert(id.clone());
            }
            if current_nodes.contains_key(id) {
                changed_current.insert(id.clone());
            }
        }
    }

    let prior_reach = reverse_reach(prior, &changed_prior);
    let current_reach = reverse_reach(current, &changed_current);
    let prior_closures = closure_map(&prior_validated);
    let current_closures = closure_map(&current_validated);
    let mut impacted = BTreeSet::new();
    for id in current_nodes.keys() {
        let closure_changed = match (prior_closures.get(id), current_closures.get(id)) {
            (Some(prior_digest), Some(current_digest)) => prior_digest != current_digest,
            (None, Some(_)) => true,
            (Some(_), None) | (None, None) => false,
        };
        if current_reach.contains(id) || prior_reach.contains(id) || closure_changed {
            impacted.insert(id.clone());
        }
    }

    Ok(ImpactPlan {
        changed_prior: changed_prior.into_iter().collect(),
        changed_current: changed_current.into_iter().collect(),
        prior_reach: prior_reach.into_iter().collect(),
        current_reach: current_reach.into_iter().collect(),
        impacted: impacted.into_iter().collect(),
    })
}

pub fn digest_graph<I, K, S>(graph: &ActionGraph<I, K, S>) -> Result<GraphDigestV1, ImpactError>
where
    I: CanonicalEncode,
    K: CanonicalEncode,
    S: CanonicalEncode,
{
    let mut sink = crate::BoundedBytesSink::new(16 * 1024 * 1024);
    graph
        .encode_canonical(&mut sink)
        .map_err(|_| ImpactError::CanonicalEncoding)?;
    Ok(hash_graph(sink.bytes()))
}

fn node_map<I: Ord + Clone, K, S>(
    graph: &ActionGraph<I, K, S>,
) -> BTreeMap<I, &NodeRecord<I, K, S>> {
    graph
        .as_slice()
        .iter()
        .map(|node| (node.id().clone(), node))
        .collect()
}

fn closure_map<I: Clone + Ord>(graph: &ValidatedGraph<I>) -> BTreeMap<I, ObjectDigestV1> {
    graph
        .closures()
        .iter()
        .map(|closure| (closure.node().clone(), closure.digest()))
        .collect()
}

fn reverse_reach<I, K, S>(graph: &ActionGraph<I, K, S>, seeds: &BTreeSet<I>) -> BTreeSet<I>
where
    I: Clone + Ord,
{
    let mut dependents: BTreeMap<I, Vec<I>> = BTreeMap::new();
    for node in graph.as_slice() {
        for dependency in node.dependencies() {
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(node.id().clone());
        }
    }

    let mut reached = seeds.clone();
    let mut frontier: Vec<I> = seeds.iter().cloned().collect();
    while let Some(id) = frontier.pop() {
        if let Some(next) = dependents.get(&id) {
            for dependent in next {
                if reached.insert(dependent.clone()) {
                    frontier.push(dependent.clone());
                }
            }
        }
    }
    reached
}

impl<I: CanonicalEncode> CanonicalEncode for ImpactPlan<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"changed_current\":[")?;
        encode_ids(&self.changed_current, out)?;
        out.write(b"],\"changed_prior\":[")?;
        encode_ids(&self.changed_prior, out)?;
        out.write(b"],\"current_reach\":[")?;
        encode_ids(&self.current_reach, out)?;
        out.write(b"],\"impacted\":[")?;
        encode_ids(&self.impacted, out)?;
        out.write(b"],\"prior_reach\":[")?;
        encode_ids(&self.prior_reach, out)?;
        out.write(b"]}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrustBindingV1 {
    issuer: ImplementationIdV1,
    audience: ImplementationIdV1,
}

impl TrustBindingV1 {
    pub const fn new(issuer: ImplementationIdV1, audience: ImplementationIdV1) -> Self {
        Self { issuer, audience }
    }

    pub const fn issuer(&self) -> ImplementationIdV1 {
        self.issuer
    }

    pub const fn audience(&self) -> ImplementationIdV1 {
        self.audience
    }
}

impl CanonicalEncode for TrustBindingV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"audience\":")?;
        write_hex(out, self.audience.as_bytes())?;
        out.write(b",\"issuer\":")?;
        write_hex(out, self.issuer.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrustRootV1 {
    repository: ApplicationNamespaceV1,
    workflow: ImplementationIdV1,
    workflow_digest: ObjectDigestV1,
    producer_bindings: Box<[TrustBindingV1]>,
    disposition_registry: ObjectDigestV1,
    transition_authority: ImplementationIdV1,
    engine_promotion_authority: ImplementationIdV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TrustRootError {
    EmptyIdentity,
    UnsortedBindings { index: usize },
}

impl fmt::Display for TrustRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "trust-root error: {self:?}")
    }
}

impl std::error::Error for TrustRootError {}

impl TrustRootV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        repository: ApplicationNamespaceV1,
        workflow: ImplementationIdV1,
        workflow_digest: ObjectDigestV1,
        producer_bindings: Vec<TrustBindingV1>,
        disposition_registry: ObjectDigestV1,
        transition_authority: ImplementationIdV1,
        engine_promotion_authority: ImplementationIdV1,
    ) -> Result<Self, TrustRootError> {
        if is_zero(repository.as_bytes())
            || is_zero(workflow.as_bytes())
            || is_zero(workflow_digest.as_bytes())
            || is_zero(disposition_registry.as_bytes())
            || is_zero(transition_authority.as_bytes())
            || is_zero(engine_promotion_authority.as_bytes())
            || producer_bindings.iter().any(|binding| {
                is_zero(binding.issuer.as_bytes()) || is_zero(binding.audience.as_bytes())
            })
        {
            return Err(TrustRootError::EmptyIdentity);
        }
        if producer_bindings.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = producer_bindings
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(TrustRootError::UnsortedBindings { index });
        }
        Ok(Self {
            repository,
            workflow,
            workflow_digest,
            producer_bindings: producer_bindings.into_boxed_slice(),
            disposition_registry,
            transition_authority,
            engine_promotion_authority,
        })
    }

    pub const fn repository(&self) -> ApplicationNamespaceV1 {
        self.repository
    }

    pub const fn workflow(&self) -> ImplementationIdV1 {
        self.workflow
    }

    pub const fn workflow_digest(&self) -> ObjectDigestV1 {
        self.workflow_digest
    }

    pub fn producer_bindings(&self) -> &[TrustBindingV1] {
        &self.producer_bindings
    }

    pub const fn disposition_registry(&self) -> ObjectDigestV1 {
        self.disposition_registry
    }

    pub const fn transition_authority(&self) -> ImplementationIdV1 {
        self.transition_authority
    }

    pub const fn engine_promotion_authority(&self) -> ImplementationIdV1 {
        self.engine_promotion_authority
    }
}

impl CanonicalEncode for TrustRootV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"disposition_registry\":")?;
        write_hex(out, self.disposition_registry.as_bytes())?;
        out.write(b",\"engine_promotion_authority\":")?;
        write_hex(out, self.engine_promotion_authority.as_bytes())?;
        out.write(b",\"producer_bindings\":[")?;
        encode_values(&self.producer_bindings, out)?;
        out.write(b"],\"repository\":")?;
        write_hex(out, self.repository.as_bytes())?;
        out.write(b",\"transition_authority\":")?;
        write_hex(out, self.transition_authority.as_bytes())?;
        out.write(b",\"workflow\":")?;
        write_hex(out, self.workflow.as_bytes())?;
        out.write(b",\"workflow_digest\":")?;
        write_hex(out, self.workflow_digest.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TransitionChangeV1<I> {
    NodeAdded(I),
    NodeRemoved(I),
    NodeChanged(I),
    DependencyChanged(I),
    OwnerNarrowing(I),
    InventoryChanged,
    TrustPolicyChanged,
}

impl<I> TransitionChangeV1<I> {
    fn is_narrowing(&self) -> bool {
        matches!(
            self,
            Self::OwnerNarrowing(_) | Self::InventoryChanged | Self::TrustPolicyChanged
        )
    }
}

impl<I: CanonicalEncode> CanonicalEncode for TransitionChangeV1<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"kind\":")?;
        let name = match self {
            Self::NodeAdded(_) => "node_added",
            Self::NodeRemoved(_) => "node_removed",
            Self::NodeChanged(_) => "node_changed",
            Self::DependencyChanged(_) => "dependency_changed",
            Self::OwnerNarrowing(_) => "owner_narrowing",
            Self::InventoryChanged => "inventory_changed",
            Self::TrustPolicyChanged => "trust_policy_changed",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)?;
        match self {
            Self::NodeAdded(id)
            | Self::NodeRemoved(id)
            | Self::NodeChanged(id)
            | Self::DependencyChanged(id)
            | Self::OwnerNarrowing(id) => {
                out.write(b",\"node\":")?;
                id.encode_canonical(out)?;
            }
            Self::InventoryChanged | Self::TrustPolicyChanged => {}
        }
        out.write(b"}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransitionApprovalV1 {
    issuer: ImplementationIdV1,
    receipt: AuthorityReceiptDigestV1,
}

impl TransitionApprovalV1 {
    pub const fn new(issuer: ImplementationIdV1, receipt: AuthorityReceiptDigestV1) -> Self {
        Self { issuer, receipt }
    }

    pub const fn issuer(&self) -> ImplementationIdV1 {
        self.issuer
    }

    pub const fn receipt(&self) -> AuthorityReceiptDigestV1 {
        self.receipt
    }
}

impl CanonicalEncode for TransitionApprovalV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"issuer\":")?;
        write_hex(out, self.issuer.as_bytes())?;
        out.write(b",\"receipt\":")?;
        write_hex(out, self.receipt.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphTransitionV1<I> {
    prior: Option<GraphDigestV1>,
    current: GraphDigestV1,
    changes: Box<[TransitionChangeV1<I>]>,
    approval: Option<TransitionApprovalV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TransitionError {
    UnsortedChanges { index: usize },
    GenesisApproval,
    GenesisRemoval,
    UnexpectedApproval,
    CandidateSelfApproval,
    InvalidApprovalIssuer,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "graph transition error: {self:?}")
    }
}

impl std::error::Error for TransitionError {}

impl<I: Ord> GraphTransitionV1<I> {
    pub fn try_new(
        prior: Option<GraphDigestV1>,
        current: GraphDigestV1,
        changes: Vec<TransitionChangeV1<I>>,
        approval: Option<TransitionApprovalV1>,
    ) -> Result<Self, TransitionError> {
        if changes.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = changes
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(TransitionError::UnsortedChanges { index });
        }
        if prior.is_none() {
            if approval.is_some() {
                return Err(TransitionError::GenesisApproval);
            }
            if changes.iter().any(|change| {
                matches!(
                    change,
                    TransitionChangeV1::NodeRemoved(_)
                        | TransitionChangeV1::OwnerNarrowing(_)
                        | TransitionChangeV1::InventoryChanged
                        | TransitionChangeV1::TrustPolicyChanged
                )
            }) {
                return Err(TransitionError::GenesisRemoval);
            }
        }
        Ok(Self {
            prior,
            current,
            changes: changes.into_boxed_slice(),
            approval,
        })
    }

    pub const fn prior(&self) -> Option<GraphDigestV1> {
        self.prior
    }

    pub const fn current(&self) -> GraphDigestV1 {
        self.current
    }

    pub fn changes(&self) -> &[TransitionChangeV1<I>] {
        &self.changes
    }

    pub const fn approval(&self) -> Option<TransitionApprovalV1> {
        self.approval
    }
}

impl<I: CanonicalEncode> CanonicalEncode for GraphTransitionV1<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"approval\":")?;
        match self.approval {
            Some(approval) => approval.encode_canonical(out)?,
            None => out.write(b"null")?,
        }
        out.write(b",\"changes\":[")?;
        encode_values(&self.changes, out)?;
        out.write(b"],\"current\":")?;
        write_hex(out, self.current.as_bytes())?;
        out.write(b",\"prior\":")?;
        match self.prior {
            Some(prior) => write_hex(out, prior.as_bytes())?,
            None => out.write(b"null")?,
        }
        out.write(b"}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TransitionDecisionV1 {
    Genesis,
    Approved,
    ConservativeSuperset,
}

pub fn validate_graph_transition<I: Ord>(
    transition: &GraphTransitionV1<I>,
    trust: &TrustRootV1,
    candidate_issuer: ImplementationIdV1,
) -> Result<TransitionDecisionV1, TransitionError> {
    if transition.prior.is_none() {
        return Ok(TransitionDecisionV1::Genesis);
    }
    let narrowing = transition
        .changes
        .iter()
        .any(TransitionChangeV1::is_narrowing);
    match (narrowing, transition.approval) {
        (false, None) => Ok(TransitionDecisionV1::Approved),
        (false, Some(_)) => Err(TransitionError::UnexpectedApproval),
        (true, None) => Ok(TransitionDecisionV1::ConservativeSuperset),
        (true, Some(approval)) if approval.issuer == candidate_issuer => {
            Err(TransitionError::CandidateSelfApproval)
        }
        (true, Some(approval)) if approval.issuer != trust.transition_authority => {
            Err(TransitionError::InvalidApprovalIssuer)
        }
        (true, Some(_)) => Ok(TransitionDecisionV1::Approved),
    }
}

fn encode_ids<I: CanonicalEncode, S: CanonicalSink>(
    ids: &[I],
    out: &mut S,
) -> Result<(), CanonicalError> {
    for (index, id) in ids.iter().enumerate() {
        if index != 0 {
            out.write(b",")?;
        }
        id.encode_canonical(out)?;
    }
    Ok(())
}

fn encode_values<T: CanonicalEncode, S: CanonicalSink>(
    values: &[T],
    out: &mut S,
) -> Result<(), CanonicalError> {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.write(b",")?;
        }
        value.encode_canonical(out)?;
    }
    Ok(())
}

fn write_hex<S: CanonicalSink>(out: &mut S, bytes: &[u8]) -> Result<(), CanonicalError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.write(b"\"")?;
    for byte in bytes {
        out.write(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    out.write(b"\"")
}

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}
