# FCI-4c: paired impact and protected transition

Status: **ready (FCI-4c v1; non-authoritative shadow)**.

This packet derives exact impact from a validated prior/current graph pair and
models the protected transition that governs ownership and inventory changes.
It is pure data and graph algebra: it does not inspect a source tree, consult a
cache, execute an action, publish an outcome, or trust a candidate-provided
approval. The H2 adapter and its source-snapshot provider remain future
consumers.

## Base and allowed paths

- Trusted base: the closed FCI-4b commit recorded in
  `ratchets/fci-readiness/fci-4c.v1.json`.
- Allowed source paths: `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/impact.rs`, its impact/transition tests, the synthetic
  `impact-cases.v1.json` fixture, this packet, the slice index, and its
  readiness envelope.
- Forbidden: source snapshot/filesystem providers, scheduler/runner effects,
  cache/CAS/outcome/publication, H2/workspace adapters, compiler/oracle/xtask,
  workflows, and candidate-controlled authority.

## Frozen API

```rust
pub struct ImpactPlan<I> {
    /* sorted changed_prior, changed_current, prior_reach, current_reach,
       and current impacted ids */
}

pub fn compare_graphs<I, K, S>(
    prior: &ActionGraph<I, K, S>,
    current: &ActionGraph<I, K, S>,
) -> Result<ImpactPlan<I>, ImpactError>
where
    I: Clone + Ord + CanonicalEncode,
    K: Eq,
    S: Eq;

pub struct TrustBindingV1 { /* protected producer issuer/audience */ }
pub struct TrustRootV1 {
    /* repository namespace, protected workflow identity/digest, producer
       bindings, disposition registry digest, transition authority, and
       engine-promotion authority */
}

pub enum TransitionChangeV1<I> {
    NodeAdded(I), NodeRemoved(I), NodeChanged(I), DependencyChanged(I),
    OwnerNarrowing(I), InventoryChanged, TrustPolicyChanged,
}

pub struct TransitionApprovalV1 { /* issuer and authority receipt digest */ }
pub struct GraphTransitionV1<I> { /* prior/current graph digests and changes */ }

pub enum TransitionDecisionV1 {
    Genesis,
    Approved,
    ConservativeSuperset,
}

pub fn validate_graph_transition<I: Ord>(
    transition: &GraphTransitionV1<I>,
    trust: &TrustRootV1,
    candidate_issuer: ImplementationIdV1,
) -> Result<TransitionDecisionV1, TransitionError>;
```

`compare_graphs` validates both sides before deriving any set. A node is
changed when presence, class, typed kind/spec, or dependency set differs. It
computes both reverse dependency closures, so a removed prior node and an
added current node cannot disappear from the calculation. `impacted` is the
current-node projection of both reaches plus any current node whose validated
closure digest changed. All five sets are strict, sorted, and duplicate-free;
the result has no path-expression or VCS shortcut.

`TrustRootV1` is a protected value, not a capability. Its producer bindings and
authority identities are exact typed values with deterministic ordering.
`GraphTransitionV1` records genesis or the exact prior/current graph digest,
the sorted structural/inventory/trust changes, and an optional authority
receipt. A genesis transition cannot carry a prior or approval. A narrowing or
trust/inventory change without a protected approval returns
`ConservativeSuperset`, allowing a later adapter to retain the validated
prior/current owner union. An approval is accepted only when its issuer is the
trust-root transition authority and is rejected when the candidate issuer
matches it; a candidate cannot approve its own narrowing. Wrong issuers and
unexpected approvals fail closed.

Canonical rendering is bounded and pure for the impact plan, trust root, and
transition records. No constructor creates a verified root, execution,
outcome, cache candidate, or authority capability.

## Proof

```text
cargo xtask test ci-core
cargo test -p tsc-rs-ci-core --test impact
node .github/ci/slice-readiness.mjs --check fci-4c
```

Fixtures cover node/edge add/remove, shared-owner reverse reach, no-impact,
closure changes, approved and unapproved narrowing, genesis, and candidate
self-approval. `impact-cases.v1.json` is synthetic only; it contains no H2
case corpus, repository path, provider locator, or cache availability.
