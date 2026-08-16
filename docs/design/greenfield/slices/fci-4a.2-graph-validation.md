# FCI-4a.2: graph and model structural validation

Status: **ready (FCI-4a.2 v1; non-authoritative shadow)**.

This packet validates an already decoded generic graph. It checks dependency
references, duplicate/self edges, cycles, stable topological order, transitive
closure members and digest equality, declared closure completeness, and global
id collisions. It also adds inert root/action/execution/derived proposal
records. It does not register an adapter, dispatch a callback, construct a
prepared execution, or claim complete membership.

## Base and allowed paths

- Trusted base: the closed FCI-4a.1 commit recorded in
  `ratchets/fci-readiness/fci-4a.2.v1.json`.
- Allowed source paths: `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/graph_validation.rs`, its validation tests, this packet,
  the slice index, and its readiness envelope.
- Forbidden: codec/registry/adapter traits, prepared execution, complete
  membership, outcomes, effects, authority, and all repository/compiler/xtask
  paths.

## Frozen API

```rust
pub enum GraphValidationError {
    MissingDependency { node_index: usize, dependency_index: usize },
    DuplicateDependency { node_index: usize, dependency_index: usize },
    SelfDependency { node_index: usize },
    Cycle,
    ClosureEncoding,
    MissingClosure { node_index: usize },
    ExtraClosure,
    StaleClosure { node_index: usize },
    GlobalIdCollision { set_index: usize, item_index: usize },
}

pub struct EvaluationPlan<I> { /* stable topological order */ }
pub struct ClosureRecord<I> { /* node, sorted transitive members, digest */ }
pub struct ValidatedGraph<I> { /* plan plus closure records */ }

pub fn validate_graph<I, K, S>(
    graph: &ActionGraph<I, K, S>,
) -> Result<ValidatedGraph<I>, GraphValidationError>
where
    I: Clone + Ord + CanonicalEncode;

pub fn validate_declared_closures<I, K, S>(
    graph: &ActionGraph<I, K, S>,
    declared: &[ClosureRecord<I>],
) -> Result<(), GraphValidationError>;

pub fn validate_global_id_sets<I: Ord>(
    sets: &[&[I]],
) -> Result<(), GraphValidationError>;

pub struct RootProposal<I, R> { /* spec and members */ }
pub struct ActionProposal<I, A> { /* id and spec */ }
pub struct ExecutionProposal<I, E> { /* id and spec */ }
pub struct DerivedProposal<I, D> { /* id and spec */ }
```

Validation uses a stable `Ord` ready set, so independent nodes always receive
the same topological order. Each closure contains the node and its complete
transitive dependency set sorted by id; its bytes are bounded canonical arrays
and its digest is an `ObjectDigestV1`. Declared closures must have exactly one
record per node and must match both members and digest. A missing edge, cycle,
duplicate edge, stale/missing/extra closure, or cross-set id collision fails
before any evaluation proposal can be consumed.

The proposal records only store typed values. They have no adapter callback,
`Any`/downcast branch, registry authority, execution constructor, or
membership-completion conversion.

## Proof

```text
cargo test -p tsc-rs-ci-core --test contracts graph_validation
node .github/ci/slice-readiness.mjs --check fci-4a.2
```

Fixtures cover stable transitive closures, missing/duplicate/self edges,
cycles, stale closure bytes, global collisions, and generic proposal data.
