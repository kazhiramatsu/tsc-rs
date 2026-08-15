# FCI-5a: tsc-rs protocol, protected control, and fixed plan

Status: **ready (FCI-5a v1; non-authoritative shadow)**.

This packet introduces the first application-side boundary without registering
an H2 action model. The protocol crate owns typed invocation, bounded
observation, root-receipt, shard-range, and fixed-plan values. The control
crate owns plan validation and composition-only witnesses. The candidate
compiler, oracle, verifier, outcome, cache, and authority remain outside both
crates.

## Base and allowed paths

- Trusted base: the closed FCI-4d commit recorded in
  `ratchets/fci-readiness/fci-5a.v1.json`.
- Allowed source paths: workspace membership/dependency aliases, the two new
  protocol/control packages and tests, the checked-in fixed-plan contract and
  resource, this packet, the slice index, and its readiness envelope.
- Forbidden: production/compiler/oracle/harness code, candidate execution,
  `ActionModel`/`AdapterCodec` registration, outcomes/projections, CAS/cache,
  workflows, and changes to H2.5g authoritative commands.

## Frozen package boundary

```text
crates/ci-adapter-tsc-rs-protocol/  -> ci-core only
crates/ci-adapter-tsc-rs-control/    -> protocol + ci-core only
```

Both packages are private, `publish = false`, and contain no dependency on a
production/compiler/oracle crate. The control package is protected composition
data; it does not call `xtask` or execute a candidate action.

## Frozen API

```rust
pub struct ActionInvocationV1 { /* action/schema/implementation/input,
                                    invocation/source/repetition/limits */ }
pub struct ObservationEnvelopeV1 { /* action/schema/implementation,
                                      bounded canonical bytes */ }
pub struct RootReceiptV1 { /* graph/profile/root/outcome/membership digests */ }

pub struct ShardRangeV1 { /* non-empty half-open range */ }
pub struct ShardSpecV1 { /* stable id, range, case-id digest */ }
pub struct FixedPlanV1 { /* exact denominator, ordered shards, membership,
                             qualification and policy digests */ }

pub struct VerifiedPlanV1 { /* private validated plan witness */ }
pub fn verify_plan(plan: FixedPlanV1) -> Result<VerifiedPlanV1, PlanError>;
```

Invocation construction rejects empty typed identities, zero output limits,
and invalid repetition/attempt values. Observation construction enforces one
bounded byte ceiling before accepting bytes and stores no executable object or
semantic verdict. Root receipts are evidence values only and cannot mint an
outcome capability.

The fixed H2 profile resource is deliberately a compact projection over the
immutable qualified case source: it binds the qualification file digest,
exact denominator `9027`, the canonical membership digest, four fixed
non-empty contiguous ranges, each range digest, and the policy ids. The plan
does not duplicate 9,027 case rows or invent a second case corpus. A future
control adapter may expand the exact source under its own packet; the generic
framework sees only the typed plan witness and has no H2 branch.

`verify_plan` checks non-empty ranges, contiguous coverage, strict shard and
policy ordering, exact denominator, and nonzero digests. It has no callback,
downcast, candidate registration, cache lookup, process spawn, or authority
constructor. `xtask` remains a legacy forwarding/composition owner until the
later H2 packets; this packet does not change it.

## Proof

```text
cargo xtask test ci-adapter-tsc-rs-protocol
cargo xtask test ci-adapter-tsc-rs-control
cargo test -p tsc-rs-ci-adapter-control --test plan
cargo tree -p tsc-rs-ci-adapter-control --edges normal,build,dev
node .github/ci/slice-readiness.mjs --check fci-5a
```

Fixtures cover typed invocation/observation limits, exact plan coverage,
duplicate/gap/empty-range rejection, canonical replay, the 9,027 source
binding, and negative dependency/source audits. No H2 action registration or
candidate process is available to the tests.
