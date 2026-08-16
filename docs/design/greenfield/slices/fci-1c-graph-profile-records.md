# FCI-1c: graph, profile, and membership record seam

Status: **ready (FCI-1c v1; non-authoritative shadow)**.

This packet declares generic records and the pending/complete membership
typestate. It does not evaluate a graph, recompute a closure, decode an
adapter, verify an observation, aggregate an outcome, or perform an effect.

## Base and allowed paths

- Trusted base: the closed FCI-1b commit recorded in
  `ratchets/fci-readiness/fci-1c.v1.json`.
- Allowed source paths: `Cargo.toml`, `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/graph.rs`, `crates/ci-core/tests/graph.rs`, this packet,
  the slice index, and its readiness envelope.
- Forbidden: codecs, registries, runner/effect crates, production/compiler/
  oracle/xtask paths, H2 profile/qualification paths, and any outcome or
  authority type.

## Frozen API

```rust
pub enum NodeClass { Input, Executable, Derived, Aggregate }

pub struct NodeRecord<I, K, S> {
    pub fn new(id: I, class: NodeClass, kind: K, spec: S, dependencies: Vec<I>) -> Self;
    pub fn id(&self) -> &I;
    pub fn class(&self) -> NodeClass;
    pub fn kind(&self) -> &K;
    pub fn spec(&self) -> &S;
    pub fn dependencies(&self) -> &[I];
}

pub struct ActionRecord<I, A> { /* id/spec/dependencies, inert */ }
pub struct RootRecord<I, R> { /* spec/members, inert */ }

pub struct InstanceIdV1([u8; 16]);
pub struct AdapterInstanceRefV1 { /* instance/adapter/schema */ }
pub struct CompositeProfileV1 { /* ordered adapter references */ }

pub struct PendingMembership<I, V> { /* expected ids; not complete */ }
pub struct CompleteMembership<I, V> { /* private sealed construction */ }
```

The record constructors only store values and preserve declaration order.
`CompleteMembership` has no public constructor or conversion from a `Vec`;
only a later FCI-4a.3 membership verifier may construct it inside the crate.
The profile/reference records have no adapter callback or string dispatch.

## Proof

```text
cargo test -p tsc-rs-ci-core --test graph
node .github/ci/slice-readiness.mjs --check fci-1c
```

Tests cover both an H2-shaped generic record and a flat record shape, while
confirming that pending values expose no complete constructor. Graph cycles,
closure, and evaluation are explicitly outside this packet.
