# FCI-2b: bounded effect-result seam

Status: **ready (FCI-2b v1; non-authoritative shadow)**.

This packet extends `ci-runner` with a bounded synchronous chunk source, an
effect-result sum, and an invocation-private staging vocabulary. It does not
add a scheduler, worker, source snapshot, action invocation, sandbox, resource
policy, staging publication, CAS/cache, or live runner entry.

## Base and allowed paths

- Trusted base: the closed FCI-2a commit recorded in
  `ratchets/fci-readiness/fci-2b.v1.json`.
- Allowed source paths: `Cargo.lock`, `crates/ci-runner/src/lib.rs`,
  `crates/ci-runner/src/bounded.rs`, its bounded-effect tests, this packet, the
  slice index, and its readiness envelope.
- Forbidden: every production/compiler/oracle/xtask path, `ci-core`, workflow,
  H2 profile, source-snapshot/sandbox/resource/publication/cache symbol, and
  any live evaluation entry.

## Frozen API

```rust
pub struct ByteLimit(u64);

impl ByteLimit {
    pub fn try_new(bytes: u64) -> Result<Self, InfraError>;
    pub const fn get(self) -> u64;
}

pub struct BoundedChunk(Box<[u8]>);

impl BoundedChunk {
    pub fn try_new(bytes: Vec<u8>, limit: ByteLimit) -> Result<Self, InfraError>;
    pub fn as_bytes(&self) -> &[u8];
}

pub trait ChunkSource {
    fn next_chunk(&mut self, limit: ByteLimit)
        -> EffectResult<Option<BoundedChunk>>;
}

pub enum EffectResult<T> {
    Complete(T),
    Failed(InfraError),
}

impl<T> EffectResult<T> {
    pub fn into_result(self) -> Result<T, InfraError>;
    pub const fn is_failed(&self) -> bool;
}
```

`ByteLimit::try_new(0)` fails with the closed `Quota` infrastructure family.
`BoundedChunk` accepts only bytes at or below the supplied limit and never
truncates. `ChunkSource::next_chunk` uses `Complete(None)` for EOF; EOF is not
a cache miss or semantic rejection. `EffectResult::Failed` carries only
`InfraError`, with no implicit retry, miss, or model-data variant.

The crate-private `StagingBuffer` owns an explicit byte ceiling, appends only
validated chunks, and clears all bytes on explicit abandon, quota overflow, or
later append after abandon. It exposes no partial contents and is not a
publication or cache handle. Its construction and ownership remain private
until a later invocation packet defines them.

## Proof

```text
cargo test -p tsc-rs-ci-runner --test bounded_effect
cargo test -p tsc-rs-ci-runner --lib
node .github/ci/slice-readiness.mjs --check fci-2b
```

Tests cover bounded reads, EOF, no truncation, error projection, staging
abandon/clear, and the absence of a miss conversion. The existing dependency
tree from FCI-2a remains unchanged: `ci-runner` depends only on `ci-core`.
