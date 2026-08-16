# FCI-1b: inert adapter descriptors

Status: **ready (FCI-1b v1; non-authoritative shadow)**.

This packet adds only generic, inert adapter identity and descriptor records to
`ci-core`. It does not add a codec, registry, callback, decoder, graph, effect,
or repository adapter.

## Base and allowed paths

- Trusted base: the closed FCI-1a commit recorded in
  `ratchets/fci-readiness/fci-1b.v1.json`.
- Allowed source paths: `Cargo.toml`, `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/adapter.rs`, `crates/ci-core/tests/descriptors.rs`, this
  packet, and its readiness envelope.
- Forbidden: every production/compiler/oracle/xtask path, all workflow and H2
  profile paths, and any FCI-1c or later symbol.

## Frozen API

```rust
pub struct AdapterIdV1([u8; 16]);

pub struct AdapterDescriptorV1 {
    adapter: AdapterIdV1,
    schema: SchemaIdV1,
    implementation: ImplementationIdV1,
}

pub struct AdapterDescriptorSetV1 {
    entries: Box<[AdapterDescriptorV1]>,
}

pub enum AdapterDescriptorError {
    EmptyIdentity,
    Unsorted { index: usize },
}

impl AdapterIdV1 {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self;
    pub const fn as_bytes(&self) -> &[u8; 16];
}

impl AdapterDescriptorV1 {
    pub fn try_new(
        adapter: AdapterIdV1,
        schema: SchemaIdV1,
        implementation: ImplementationIdV1,
    ) -> Result<Self, AdapterDescriptorError>;
    pub const fn adapter(&self) -> AdapterIdV1;
    pub const fn schema(&self) -> SchemaIdV1;
    pub const fn implementation(&self) -> ImplementationIdV1;
}

impl AdapterDescriptorSetV1 {
    pub fn try_from_sorted(
        entries: Vec<AdapterDescriptorV1>,
    ) -> Result<Self, AdapterDescriptorError>;
    pub fn as_slice(&self) -> &[AdapterDescriptorV1];
}
```

The descriptor constructor rejects an all-zero adapter, schema, or
implementation identity. The set constructor requires strict `Ord` order,
which rejects duplicates without interpreting an adapter id. The records have
no serialization or hashing behavior until FCI-3a.

## Proof

```text
cargo test -p tsc-rs-ci-core --test contracts descriptors
node .github/ci/slice-readiness.mjs --check fci-1b
```

The proof must show empty identities, duplicate/unsorted descriptors, and
repository/compiler literal audits fail closed. No registration or runtime
callback may be reachable from the public API.
