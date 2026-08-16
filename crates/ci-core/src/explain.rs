use core::fmt;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    ActionGraph, CanonicalEncode, CanonicalError, CanonicalSink, GraphValidationError,
    ObjectDigestV1,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExplanationError {
    Graph(GraphValidationError),
    Unsorted { index: usize },
    Duplicate { index: usize },
    Overlap { index: usize },
    MissingTarget,
    NoReasonPath,
    TargetMismatch,
}

impl fmt::Display for ExplanationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "explanation error: {self:?}")
    }
}

impl std::error::Error for ExplanationError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlanSets<I> {
    changed: Box<[I]>,
    impacted: Box<[I]>,
    carry_forward: Box<[I]>,
    cache_reuse: Box<[I]>,
    execute: Box<[I]>,
    revalidate: Box<[I]>,
    repack: Box<[I]>,
    rebuild: Box<[I]>,
}

impl<I: Ord> PlanSets<I> {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        changed: Vec<I>,
        impacted: Vec<I>,
        carry_forward: Vec<I>,
        cache_reuse: Vec<I>,
        execute: Vec<I>,
        revalidate: Vec<I>,
        repack: Vec<I>,
        rebuild: Vec<I>,
    ) -> Result<Self, ExplanationError> {
        let values = [
            &changed,
            &impacted,
            &carry_forward,
            &cache_reuse,
            &execute,
            &revalidate,
            &repack,
            &rebuild,
        ];
        for value in values {
            strict(value)?;
        }
        for (index, left) in [&carry_forward, &cache_reuse, &execute].iter().enumerate() {
            for right in [&carry_forward, &cache_reuse, &execute]
                .iter()
                .skip(index + 1)
            {
                if left.iter().any(|item| right.binary_search(item).is_ok()) {
                    return Err(ExplanationError::Overlap { index });
                }
            }
        }
        Ok(Self {
            changed: changed.into_boxed_slice(),
            impacted: impacted.into_boxed_slice(),
            carry_forward: carry_forward.into_boxed_slice(),
            cache_reuse: cache_reuse.into_boxed_slice(),
            execute: execute.into_boxed_slice(),
            revalidate: revalidate.into_boxed_slice(),
            repack: repack.into_boxed_slice(),
            rebuild: rebuild.into_boxed_slice(),
        })
    }

    pub fn changed(&self) -> &[I] {
        &self.changed
    }

    pub fn impacted(&self) -> &[I] {
        &self.impacted
    }

    pub fn carry_forward(&self) -> &[I] {
        &self.carry_forward
    }

    pub fn cache_reuse(&self) -> &[I] {
        &self.cache_reuse
    }

    pub fn execute(&self) -> &[I] {
        &self.execute
    }

    pub fn revalidate(&self) -> &[I] {
        &self.revalidate
    }

    pub fn repack(&self) -> &[I] {
        &self.repack
    }

    pub fn rebuild(&self) -> &[I] {
        &self.rebuild
    }
}

impl<I: CanonicalEncode> CanonicalEncode for PlanSets<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"cache_reuse\":[")?;
        encode_ids(&self.cache_reuse, out)?;
        out.write(b"],\"carry_forward\":[")?;
        encode_ids(&self.carry_forward, out)?;
        out.write(b"],\"changed\":[")?;
        encode_ids(&self.changed, out)?;
        out.write(b"],\"execute\":[")?;
        encode_ids(&self.execute, out)?;
        out.write(b"],\"impacted\":[")?;
        encode_ids(&self.impacted, out)?;
        out.write(b"],\"rebuild\":[")?;
        encode_ids(&self.rebuild, out)?;
        out.write(b"],\"repack\":[")?;
        encode_ids(&self.repack, out)?;
        out.write(b"],\"revalidate\":[")?;
        encode_ids(&self.revalidate, out)?;
        out.write(b"]}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReasonPath<I> {
    target: I,
    path: Box<[I]>,
}

impl<I: Ord> ReasonPath<I> {
    pub fn try_new(target: I, path: Vec<I>) -> Result<Self, ExplanationError> {
        if path.last() != Some(&target) {
            return Err(ExplanationError::TargetMismatch);
        }
        Ok(Self {
            target,
            path: path.into_boxed_slice(),
        })
    }

    pub fn target(&self) -> &I {
        &self.target
    }

    pub fn path(&self) -> &[I] {
        &self.path
    }
}

impl<I: CanonicalEncode> CanonicalEncode for ReasonPath<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"path\":[")?;
        encode_ids(&self.path, out)?;
        out.write(b"],\"target\":")?;
        self.target.encode_canonical(out)?;
        out.write(b"}")
    }
}

pub fn shortest_reason_paths<I, K, S>(
    graph: &ActionGraph<I, K, S>,
    impacted: &[I],
) -> Result<Box<[ReasonPath<I>]>, ExplanationError>
where
    I: Clone + Ord + CanonicalEncode,
{
    crate::validate_graph(graph).map_err(ExplanationError::Graph)?;
    strict(impacted)?;
    let nodes = graph.as_slice();
    let known: BTreeSet<I> = nodes.iter().map(|node| node.id().clone()).collect();
    if impacted.iter().any(|id| !known.contains(id)) {
        return Err(ExplanationError::MissingTarget);
    }
    let mut reverse: BTreeMap<I, Vec<I>> = BTreeMap::new();
    let mut roots = Vec::new();
    for node in nodes {
        if node.dependencies().is_empty() {
            roots.push(node.id().clone());
        }
        for dependency in node.dependencies() {
            reverse
                .entry(dependency.clone())
                .or_default()
                .push(node.id().clone());
        }
    }
    roots.sort();
    for dependents in reverse.values_mut() {
        dependents.sort();
    }

    impacted
        .iter()
        .map(|target| {
            let path =
                shortest_path(target, &roots, &reverse).ok_or(ExplanationError::NoReasonPath)?;
            ReasonPath::try_new(target.clone(), path)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn shortest_path<I: Clone + Ord>(
    target: &I,
    roots: &[I],
    reverse: &BTreeMap<I, Vec<I>>,
) -> Option<Vec<I>> {
    let mut queue = VecDeque::new();
    let mut best: BTreeMap<I, Vec<I>> = BTreeMap::new();
    for root in roots {
        let path = vec![root.clone()];
        best.insert(root.clone(), path.clone());
        queue.push_back(path);
    }
    let mut answer: Option<Vec<I>> = None;
    while let Some(path) = queue.pop_front() {
        let last = path.last()?;
        if answer
            .as_ref()
            .is_some_and(|current| path.len() > current.len())
        {
            continue;
        }
        if last == target {
            if answer.as_ref().is_none_or(|current| path < *current) {
                answer = Some(path.clone());
            }
            continue;
        }
        if let Some(dependents) = reverse.get(last) {
            for dependent in dependents {
                if path.contains(dependent) {
                    continue;
                }
                let mut candidate = path.clone();
                candidate.push(dependent.clone());
                let should_enqueue = best
                    .get(dependent)
                    .is_none_or(|known| candidate.len() < known.len() || candidate < *known);
                if should_enqueue {
                    best.insert(dependent.clone(), candidate.clone());
                    queue.push_back(candidate);
                }
            }
        }
    }
    answer
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MissFieldV1 {
    Input,
    Graph,
    Implementation,
    Verifier,
    Projection,
    Availability,
}

impl CanonicalEncode for MissFieldV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let name = match self {
            Self::Input => "input",
            Self::Graph => "graph",
            Self::Implementation => "implementation",
            Self::Verifier => "verifier",
            Self::Projection => "projection",
            Self::Availability => "availability",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MissDifferenceV1 {
    field: MissFieldV1,
    expected: ObjectDigestV1,
    available: Option<ObjectDigestV1>,
}

impl MissDifferenceV1 {
    pub const fn new(
        field: MissFieldV1,
        expected: ObjectDigestV1,
        available: Option<ObjectDigestV1>,
    ) -> Self {
        Self {
            field,
            expected,
            available,
        }
    }

    pub const fn field(&self) -> MissFieldV1 {
        self.field
    }

    pub const fn expected(&self) -> ObjectDigestV1 {
        self.expected
    }

    pub const fn available(&self) -> Option<ObjectDigestV1> {
        self.available
    }
}

impl CanonicalEncode for MissDifferenceV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"available\":")?;
        match self.available {
            Some(digest) => write_hex(out, digest.as_bytes())?,
            None => out.write(b"null")?,
        }
        out.write(b",\"expected\":")?;
        write_hex(out, self.expected.as_bytes())?;
        out.write(b",\"field\":")?;
        self.field.encode_canonical(out)?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WhyMiss<I> {
    action: I,
    difference: MissDifferenceV1,
    reason: ReasonPath<I>,
}

impl<I: Ord> WhyMiss<I> {
    pub fn try_new(
        action: I,
        difference: MissDifferenceV1,
        reason: ReasonPath<I>,
    ) -> Result<Self, ExplanationError> {
        if reason.target() != &action {
            return Err(ExplanationError::TargetMismatch);
        }
        Ok(Self {
            action,
            difference,
            reason,
        })
    }

    pub fn action(&self) -> &I {
        &self.action
    }

    pub const fn difference(&self) -> MissDifferenceV1 {
        self.difference
    }

    pub fn reason(&self) -> &ReasonPath<I> {
        &self.reason
    }
}

impl<I: CanonicalEncode> CanonicalEncode for WhyMiss<I> {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"action\":")?;
        self.action.encode_canonical(out)?;
        out.write(b",\"difference\":")?;
        self.difference.encode_canonical(out)?;
        out.write(b",\"reason\":")?;
        self.reason.encode_canonical(out)?;
        out.write(b"}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BudgetFieldV1 {
    ControlCpuMillis,
    ResidentBytes,
    GraphBytes,
    InventoryBytes,
    HashedBytes,
    DecodedBytes,
    ExplanationBytes,
    Concurrency,
}

impl CanonicalEncode for BudgetFieldV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let name = match self {
            Self::ControlCpuMillis => "control_cpu_millis",
            Self::ResidentBytes => "resident_bytes",
            Self::GraphBytes => "graph_bytes",
            Self::InventoryBytes => "inventory_bytes",
            Self::HashedBytes => "hashed_bytes",
            Self::DecodedBytes => "decoded_bytes",
            Self::ExplanationBytes => "explanation_bytes",
            Self::Concurrency => "concurrency",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlanningBudgetV1 {
    control_cpu_millis: u64,
    resident_bytes: u64,
    graph_bytes: u64,
    inventory_bytes: u64,
    hashed_bytes: u64,
    decoded_bytes: u64,
    explanation_bytes: u64,
    concurrency: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BudgetError {
    ZeroCeiling(BudgetFieldV1),
    Exceeded(BudgetFieldV1),
}

impl fmt::Display for BudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "budget error: {self:?}")
    }
}

impl std::error::Error for BudgetError {}

impl PlanningBudgetV1 {
    #[allow(clippy::too_many_arguments)]
    pub const fn try_new(
        control_cpu_millis: u64,
        resident_bytes: u64,
        graph_bytes: u64,
        inventory_bytes: u64,
        hashed_bytes: u64,
        decoded_bytes: u64,
        explanation_bytes: u64,
        concurrency: u32,
    ) -> Result<Self, BudgetError> {
        if control_cpu_millis == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::ControlCpuMillis));
        }
        if resident_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::ResidentBytes));
        }
        if graph_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::GraphBytes));
        }
        if inventory_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::InventoryBytes));
        }
        if hashed_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::HashedBytes));
        }
        if decoded_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::DecodedBytes));
        }
        if explanation_bytes == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::ExplanationBytes));
        }
        if concurrency == 0 {
            return Err(BudgetError::ZeroCeiling(BudgetFieldV1::Concurrency));
        }
        Ok(Self {
            control_cpu_millis,
            resident_bytes,
            graph_bytes,
            inventory_bytes,
            hashed_bytes,
            decoded_bytes,
            explanation_bytes,
            concurrency,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlanningObservationV1 {
    control_cpu_millis: u64,
    resident_bytes: u64,
    graph_bytes: u64,
    inventory_bytes: u64,
    hashed_bytes: u64,
    decoded_bytes: u64,
    explanation_bytes: u64,
    concurrency: u32,
}

impl PlanningObservationV1 {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        control_cpu_millis: u64,
        resident_bytes: u64,
        graph_bytes: u64,
        inventory_bytes: u64,
        hashed_bytes: u64,
        decoded_bytes: u64,
        explanation_bytes: u64,
        concurrency: u32,
    ) -> Self {
        Self {
            control_cpu_millis,
            resident_bytes,
            graph_bytes,
            inventory_bytes,
            hashed_bytes,
            decoded_bytes,
            explanation_bytes,
            concurrency,
        }
    }
}

pub fn validate_budget(
    budget: &PlanningBudgetV1,
    observation: &PlanningObservationV1,
) -> Result<(), BudgetError> {
    let checks = [
        (
            BudgetFieldV1::ControlCpuMillis,
            observation.control_cpu_millis > budget.control_cpu_millis,
        ),
        (
            BudgetFieldV1::ResidentBytes,
            observation.resident_bytes > budget.resident_bytes,
        ),
        (
            BudgetFieldV1::GraphBytes,
            observation.graph_bytes > budget.graph_bytes,
        ),
        (
            BudgetFieldV1::InventoryBytes,
            observation.inventory_bytes > budget.inventory_bytes,
        ),
        (
            BudgetFieldV1::HashedBytes,
            observation.hashed_bytes > budget.hashed_bytes,
        ),
        (
            BudgetFieldV1::DecodedBytes,
            observation.decoded_bytes > budget.decoded_bytes,
        ),
        (
            BudgetFieldV1::ExplanationBytes,
            observation.explanation_bytes > budget.explanation_bytes,
        ),
        (
            BudgetFieldV1::Concurrency,
            observation.concurrency > budget.concurrency,
        ),
    ];
    checks
        .into_iter()
        .find_map(|(field, exceeded)| exceeded.then_some(field))
        .map_or(Ok(()), |field| Err(BudgetError::Exceeded(field)))
}

impl CanonicalEncode for PlanningBudgetV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        encode_budget(
            out,
            self.control_cpu_millis,
            self.resident_bytes,
            self.graph_bytes,
            self.inventory_bytes,
            self.hashed_bytes,
            self.decoded_bytes,
            self.explanation_bytes,
            self.concurrency,
        )
    }
}

impl CanonicalEncode for PlanningObservationV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        encode_budget(
            out,
            self.control_cpu_millis,
            self.resident_bytes,
            self.graph_bytes,
            self.inventory_bytes,
            self.hashed_bytes,
            self.decoded_bytes,
            self.explanation_bytes,
            self.concurrency,
        )
    }
}

fn strict<I: Ord>(values: &[I]) -> Result<(), ExplanationError> {
    for (index, pair) in values.windows(2).enumerate() {
        if pair[0] == pair[1] {
            return Err(ExplanationError::Duplicate { index: index + 1 });
        }
        if pair[0] > pair[1] {
            return Err(ExplanationError::Unsorted { index: index + 1 });
        }
    }
    Ok(())
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

fn write_hex<S: CanonicalSink>(out: &mut S, bytes: &[u8]) -> Result<(), CanonicalError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.write(b"\"")?;
    for byte in bytes {
        out.write(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    out.write(b"\"")
}

#[allow(clippy::too_many_arguments)]
fn encode_budget<S: CanonicalSink>(
    out: &mut S,
    control_cpu_millis: u64,
    resident_bytes: u64,
    graph_bytes: u64,
    inventory_bytes: u64,
    hashed_bytes: u64,
    decoded_bytes: u64,
    explanation_bytes: u64,
    concurrency: u32,
) -> Result<(), CanonicalError> {
    out.write(b"{\"concurrency\":")?;
    write_json_u64(out, concurrency as u64)?;
    out.write(b",\"control_cpu_millis\":")?;
    write_json_u64(out, control_cpu_millis)?;
    out.write(b",\"decoded_bytes\":")?;
    write_json_u64(out, decoded_bytes)?;
    out.write(b",\"explanation_bytes\":")?;
    write_json_u64(out, explanation_bytes)?;
    out.write(b",\"graph_bytes\":")?;
    write_json_u64(out, graph_bytes)?;
    out.write(b",\"hashed_bytes\":")?;
    write_json_u64(out, hashed_bytes)?;
    out.write(b",\"inventory_bytes\":")?;
    write_json_u64(out, inventory_bytes)?;
    out.write(b",\"resident_bytes\":")?;
    write_json_u64(out, resident_bytes)?;
    out.write(b"}")
}

fn write_json_u64<S: CanonicalSink>(out: &mut S, value: u64) -> Result<(), CanonicalError> {
    let text = value.to_string();
    out.write(text.as_bytes())
}
