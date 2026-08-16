# FCI-4d: pure explanations and planning budgets

Status: **ready (FCI-4d v1; non-authoritative shadow)**.

This packet turns the immutable impact/evidence values into deterministic
explanations and a resource-envelope value. It does not perform a lookup,
spawn a process, read a clock, or query a cache. The same graph, plan sets,
candidate evidence snapshot, and miss fields therefore produce byte-identical
text/JSON-ready values.

## Base and allowed paths

- Trusted base: the closed FCI-4c commit recorded in
  `ratchets/fci-readiness/fci-4d.v1.json`.
- Allowed source paths: `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/explain.rs`, its explanation/budget tests, the synthetic
  explanation fixture, this packet, the slice index, and its readiness
  envelope.
- Forbidden: snapshot/filesystem providers, live runner/scheduler effects,
  semantic subprocesses, cache/CAS/outcome/publication, H2/workspace adapters,
  compiler/oracle/xtask, workflows, and ambient environment/time.

## Frozen API

```rust
pub struct PlanSets<I> {
    /* changed, impacted, carry_forward, cache_reuse, execute, revalidate,
       repack, and rebuild sets */
}

pub struct ReasonPath<I> { /* target and lexicographically least shortest path */ }

pub fn shortest_reason_paths<I, K, S>(
    graph: &ActionGraph<I, K, S>,
    impacted: &[I],
) -> Result<Box<[ReasonPath<I>]>, ExplanationError>;

pub enum MissFieldV1 {
    Input, Graph, Implementation, Verifier, Projection, Availability,
}

pub struct WhyMiss<I> {
    /* action, first canonical field difference, and complete reason path */
}

pub struct PlanningBudgetV1 {
    /* control CPU/RSS, graph/inventory/hash/decode/explanation bytes, and
       concurrency ceilings */
}

pub struct PlanningObservationV1 { /* measured bounded counters */ }

pub fn validate_budget(
    budget: &PlanningBudgetV1,
    observation: &PlanningObservationV1,
) -> Result<(), BudgetError>;
```

`PlanSets` requires strict sorted disjoint sets where the contract says so;
`execute` is supplied from explicit evidence availability and is never inferred
from `impacted` or shard membership. Reason paths traverse dependency edges in
the semantic direction and use breadth-first distance followed by `Ord`
tie-breaking, yielding one lexicographically least shortest path per target.
`WhyMiss` compares an expected key to a named candidate in the immutable
evidence snapshot and reports only the first field in canonical field order;
it never consults ambient candidates.

Budget values are hard ceilings, not measurements or scheduler authority.
Validation rejects zero/overflowing ceilings and any observation over CPU,
RSS, bytes, or concurrency limits. Resource baselines are checked-in synthetic
fixtures and may be tightened only by a new packet; this packet does not run a
semantic action or claim a performance result for H2.

## Proof

```text
cargo xtask test ci-core
cargo test -p tsc-rs-ci-core --test contracts explain
node .github/ci/slice-readiness.mjs --check fci-4d
```

Fixtures cover stable shortest-path tie-breaking, explicit carry-forward/cache
reuse/execute sets, first-field miss differences, replay-identical rendering,
budget boundary/overflow cases, and a negative audit proving no subprocess,
clock, environment, or repository branch is available to the pure harness.
