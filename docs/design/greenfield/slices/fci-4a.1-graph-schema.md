# FCI-4a.1: graph schema and canonical rendering

Status: **ready (FCI-4a.1 v1; non-authoritative shadow)**.

This packet adds the pure, generic action-graph schema and composite-profile
canonical rendering. It reuses the inert `NodeRecord` and canonical sink but
does not recompute closure, validate cycles/edges, dispatch adapters, prepare
executions, or construct complete membership.

## Base and allowed paths

- Trusted base: the closed FCI-3c commit recorded in
  `ratchets/fci-readiness/fci-4a.1.v1.json`.
- Allowed source paths: `crates/ci-core/src/lib.rs`, `src/canonical.rs`,
  `src/graph.rs`, `src/graph_schema.rs`, the graph-schema tests, this packet,
  the slice index, and its readiness envelope.
- Forbidden: structural closure validation, registry/codec/adapter traits,
  prepared execution, membership completion, outcome/effect/authority/cache,
  and all repository/compiler/xtask paths.

## Frozen API

```rust
pub enum GraphSchemaError {
    Unsorted { index: usize },
}

pub struct ActionGraph<I, K, S> {
    nodes: Box<[NodeRecord<I, K, S>]>,
}

impl<I: Ord, K, S> ActionGraph<I, K, S> {
    pub fn try_from_sorted(
        nodes: Vec<NodeRecord<I, K, S>>,
    ) -> Result<Self, GraphSchemaError>;
    pub fn as_slice(&self) -> &[NodeRecord<I, K, S>];
}

impl<I, K, S> CanonicalEncode for ActionGraph<I, K, S>
where
    I: CanonicalEncode,
    K: CanonicalEncode,
    S: CanonicalEncode,
{ /* canonical nodes/class/dependencies/id/kind/spec object */ }

impl CompositeProfileV1 {
    pub fn try_from_sorted(
        instances: Vec<AdapterInstanceRefV1>,
    ) -> Result<Self, GraphSchemaError>;
}
```

Node records and profile instances are strictly ordered by their opaque ids;
duplicates and unsorted inputs fail before rendering. Graph bytes use sorted
object keys and preserve dependency declaration order. Each node renders
`class`, `dependencies`, `id`, `kind`, and `spec`; the generic bounds require
the existing `CanonicalEncode` contract and do not accept a callback or an
opaque string dispatcher. Composite profile references render fixed-width
lowercase hexadecimal identity strings in stable `adapter`, `instance`,
`schema` field order.

The packet deliberately does not infer topology, recompute reverse closure,
interpret `kind`, check missing edges, or decide membership. Those invariants
belong to FCI-4a.2 and FCI-4a.3.

## Proof

```text
cargo test -p tsc-rs-ci-core --test contracts graph_schema
node .github/ci/slice-readiness.mjs --check fci-4a.1
```

Fixtures use both an H2-shaped executable graph and a flat derived graph with
the same generic records, reject duplicate/unsorted node ids, verify profile
ordering and exact bytes, and audit generic source for repository/compiler
literals and callbacks.
