use std::collections::{BTreeMap, BTreeSet};

use crate::{hash_object, ActionGraph, CanonicalEncode, CanonicalSink, NodeRecord, ObjectDigestV1};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GraphValidationError {
    MissingDependency {
        node_index: usize,
        dependency_index: usize,
    },
    DuplicateDependency {
        node_index: usize,
        dependency_index: usize,
    },
    SelfDependency {
        node_index: usize,
    },
    Cycle,
    ClosureEncoding,
    MissingClosure {
        node_index: usize,
    },
    ExtraClosure,
    StaleClosure {
        node_index: usize,
    },
    GlobalIdCollision {
        set_index: usize,
        item_index: usize,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvaluationPlan<I> {
    order: Box<[I]>,
}

impl<I> EvaluationPlan<I> {
    pub fn order(&self) -> &[I] {
        &self.order
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClosureRecord<I> {
    node: I,
    members: Box<[I]>,
    digest: ObjectDigestV1,
}

impl<I> ClosureRecord<I> {
    pub fn new(node: I, members: Vec<I>, digest: ObjectDigestV1) -> Self {
        Self {
            node,
            members: members.into_boxed_slice(),
            digest,
        }
    }

    pub fn node(&self) -> &I {
        &self.node
    }

    pub fn members(&self) -> &[I] {
        &self.members
    }

    pub const fn digest(&self) -> ObjectDigestV1 {
        self.digest
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValidatedGraph<I> {
    plan: EvaluationPlan<I>,
    closures: Box<[ClosureRecord<I>]>,
}

impl<I> ValidatedGraph<I> {
    pub fn plan(&self) -> &EvaluationPlan<I> {
        &self.plan
    }

    pub fn closures(&self) -> &[ClosureRecord<I>] {
        &self.closures
    }
}

pub fn validate_graph<I, K, S>(
    graph: &ActionGraph<I, K, S>,
) -> Result<ValidatedGraph<I>, GraphValidationError>
where
    I: Clone + Ord + CanonicalEncode,
{
    let nodes = graph.as_slice();
    let mut indices = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        indices.insert(node.id().clone(), index);
    }

    let mut dependents = vec![Vec::<usize>::new(); nodes.len()];
    let mut remaining = vec![0usize; nodes.len()];
    for (node_index, node) in nodes.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for (dependency_index, dependency) in node.dependencies().iter().enumerate() {
            let Some(&dependency_node) = indices.get(dependency) else {
                return Err(GraphValidationError::MissingDependency {
                    node_index,
                    dependency_index,
                });
            };
            if dependency_node == node_index {
                return Err(GraphValidationError::SelfDependency { node_index });
            }
            if !seen.insert(dependency_node) {
                return Err(GraphValidationError::DuplicateDependency {
                    node_index,
                    dependency_index,
                });
            }
            dependents[dependency_node].push(node_index);
            remaining[node_index] += 1;
        }
    }

    let mut ready = BTreeSet::new();
    for (index, count) in remaining.iter().enumerate() {
        if *count == 0 {
            ready.insert(index);
        }
    }
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(index) = ready.pop_first() {
        order.push(nodes[index].id().clone());
        for dependent in &dependents[index] {
            remaining[*dependent] -= 1;
            if remaining[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(GraphValidationError::Cycle);
    }

    let closures = nodes
        .iter()
        .enumerate()
        .map(|(node_index, node)| {
            let mut members = BTreeSet::new();
            collect_closure(node_index, nodes, &indices, &mut members);
            let members = members.into_iter().collect::<Vec<_>>();
            let digest = digest_ids(&members).ok_or(GraphValidationError::ClosureEncoding)?;
            Ok(ClosureRecord::new(node.id().clone(), members, digest))
        })
        .collect::<Result<Vec<_>, GraphValidationError>>()?;

    Ok(ValidatedGraph {
        plan: EvaluationPlan {
            order: order.into_boxed_slice(),
        },
        closures: closures.into_boxed_slice(),
    })
}

fn collect_closure<I, K, S>(
    node_index: usize,
    nodes: &[NodeRecord<I, K, S>],
    indices: &BTreeMap<I, usize>,
    members: &mut BTreeSet<I>,
) where
    I: Clone + Ord,
{
    if !members.insert(nodes[node_index].id().clone()) {
        return;
    }
    for dependency in nodes[node_index].dependencies() {
        if let Some(&dependency_index) = indices.get(dependency) {
            collect_closure(dependency_index, nodes, indices, members);
        }
    }
}

fn digest_ids<I: CanonicalEncode>(ids: &[I]) -> Option<ObjectDigestV1> {
    let mut sink = crate::BoundedBytesSink::new(1024 * 1024);
    sink.write(b"[").ok()?;
    for (index, id) in ids.iter().enumerate() {
        if index != 0 {
            sink.write(b",").ok()?;
        }
        id.encode_canonical(&mut sink).ok()?;
    }
    sink.write(b"]").ok()?;
    Some(hash_object(sink.bytes()))
}

pub fn validate_declared_closures<I, K, S>(
    graph: &ActionGraph<I, K, S>,
    declared: &[ClosureRecord<I>],
) -> Result<(), GraphValidationError>
where
    I: Clone + Ord + CanonicalEncode,
{
    let validated = validate_graph(graph)?;
    let mut expected = BTreeMap::new();
    for (index, closure) in validated.closures.iter().enumerate() {
        expected.insert(closure.node().clone(), (index, closure));
    }
    let mut seen = BTreeSet::new();
    for closure in declared {
        let Some((index, expected)) = expected.get(closure.node()) else {
            return Err(GraphValidationError::ExtraClosure);
        };
        if !seen.insert(closure.node().clone()) {
            return Err(GraphValidationError::ExtraClosure);
        }
        if closure.members() != expected.members() || closure.digest() != expected.digest() {
            return Err(GraphValidationError::StaleClosure { node_index: *index });
        }
    }
    if seen.len() != expected.len() {
        let missing = expected
            .keys()
            .enumerate()
            .find(|(_, id)| !seen.contains(*id))
            .map_or(0, |(index, _)| index);
        return Err(GraphValidationError::MissingClosure {
            node_index: missing,
        });
    }
    Ok(())
}

pub fn validate_global_id_sets<I: Ord>(sets: &[&[I]]) -> Result<(), GraphValidationError> {
    let mut seen = BTreeSet::new();
    for (set_index, set) in sets.iter().enumerate() {
        for (item_index, id) in set.iter().enumerate() {
            if !seen.insert(id) {
                return Err(GraphValidationError::GlobalIdCollision {
                    set_index,
                    item_index,
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RootProposal<I, R> {
    spec: R,
    members: Box<[I]>,
}

impl<I, R> RootProposal<I, R> {
    pub fn new(spec: R, members: Vec<I>) -> Self {
        Self {
            spec,
            members: members.into_boxed_slice(),
        }
    }

    pub fn spec(&self) -> &R {
        &self.spec
    }

    pub fn members(&self) -> &[I] {
        &self.members
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActionProposal<I, A> {
    id: I,
    spec: A,
}

impl<I, A> ActionProposal<I, A> {
    pub fn new(id: I, spec: A) -> Self {
        Self { id, spec }
    }

    pub fn id(&self) -> &I {
        &self.id
    }

    pub fn spec(&self) -> &A {
        &self.spec
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutionProposal<I, E> {
    id: I,
    spec: E,
}

impl<I, E> ExecutionProposal<I, E> {
    pub fn new(id: I, spec: E) -> Self {
        Self { id, spec }
    }

    pub fn id(&self) -> &I {
        &self.id
    }

    pub fn spec(&self) -> &E {
        &self.spec
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DerivedProposal<I, D> {
    id: I,
    spec: D,
}

impl<I, D> DerivedProposal<I, D> {
    pub fn new(id: I, spec: D) -> Self {
        Self { id, spec }
    }

    pub fn id(&self) -> &I {
        &self.id
    }

    pub fn spec(&self) -> &D {
        &self.spec
    }
}
