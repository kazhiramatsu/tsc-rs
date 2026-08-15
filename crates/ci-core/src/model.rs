use crate::{
    ActionGraph, ActionProposal, CanonicalEncode, DerivedProposal, ExecutionProposal, NodeRecord,
    RootProposal,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ModelError {
    UnknownNode,
    InvalidSpec,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CandidateVerificationError {
    Malformed,
    Rejected { code: u16 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterInvariantError {
    IncompleteDependencies,
    InvalidDerivedValue,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerdictCode(pub u16);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LeafVerdict<V> {
    Accepted(V),
    Rejected { code: VerdictCode },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DerivedVerdict<V> {
    Accepted(V),
    Rejected { code: VerdictCode },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterVerdict<P> {
    Accepted(P),
    Rejected { code: VerdictCode },
}

pub trait ActionModel: Sized + Send + Sync + 'static {
    type NodeId: Clone + Ord + CanonicalEncode;
    type NodeKind: CanonicalEncode;
    type NodeSpec: CanonicalEncode;
    type ActionSpec: CanonicalEncode;
    type RawObservation: CanonicalEncode;
    type VerifiedObservation: CanonicalEncode;
    type DerivedSpec: CanonicalEncode;
    type DerivedValue: CanonicalEncode;
    type RootSpec: CanonicalEncode;
    type AggregatePayload: CanonicalEncode;

    fn graph(&self) -> &ActionGraph<Self::NodeId, Self::NodeKind, Self::NodeSpec>;

    fn root_proposal(&self) -> Result<RootProposal<Self::NodeId, Self::RootSpec>, ModelError>;

    fn action_spec(
        &self,
        id: &Self::NodeId,
    ) -> Result<ActionProposal<Self::NodeId, Self::ActionSpec>, ModelError>;

    fn execution_spec(
        &self,
        id: &Self::NodeId,
    ) -> Result<ExecutionProposal<Self::NodeId, Self::ActionSpec>, ModelError>;

    fn verify_observation(
        &self,
        spec: &Self::ActionSpec,
        raw: Self::RawObservation,
    ) -> Result<LeafVerdict<Self::VerifiedObservation>, CandidateVerificationError>;

    fn derived_spec(
        &self,
        id: &Self::NodeId,
    ) -> Result<DerivedProposal<Self::NodeId, Self::DerivedSpec>, ModelError>;

    fn evaluate_derived(
        &self,
        spec: &Self::DerivedSpec,
        dependencies: &[Self::DerivedValue],
    ) -> Result<DerivedVerdict<Self::DerivedValue>, AdapterInvariantError>;

    fn aggregate(
        &self,
        complete: &[(Self::NodeId, Self::VerifiedObservation)],
    ) -> Result<AdapterVerdict<Self::AggregatePayload>, AdapterInvariantError>;
}

// Keep the type-level dependency visible in the trait without creating an
// executable registry or a second graph representation.
#[allow(dead_code)]
fn _node_record_is_the_graph_record<I, K, S>(_record: &NodeRecord<I, K, S>) {}
