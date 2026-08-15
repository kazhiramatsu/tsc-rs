# FCI-3c: source, path, sandbox, and resource primitives

Status: **ready (FCI-3c v1; non-authoritative shadow)**.

This packet completes the first effect boundary without adding CAS, cache,
publication, or live evaluation. It defines immutable snapshot descriptors,
crate-controlled verified/mounted/guard values, synchronous provider/sandbox
traits, validated relative paths, bounded regular-file reads, no-replace
staging, bounded queues, and explicit control/child resource ceilings.

## Base and allowed paths

- Trusted base: the closed FCI-3b commit recorded in
  `ratchets/fci-readiness/fci-3c.v1.json`.
- Allowed source paths: `Cargo.toml`, `Cargo.lock`, the `ci-runner` manifest,
  `src/lib.rs`, `src/resource.rs`, `src/snapshot.rs`, their tests, this packet,
  the slice index, and its readiness envelope.
- Forbidden: CAS/cache/publication, live `Runner`, H2/compiler/oracle/xtask,
  workflows, and any unreviewed process or async runtime.

## Frozen API

```rust
pub struct RelativePathV1(Box<[u8]>);
pub struct SourceSnapshotLimits {
    pub max_entries: u64,
    pub max_bytes: u64,
    pub max_path_bytes: u64,
}
pub struct SourceSnapshotRequestV1 { /* namespace/revision/provider/entries */ }
pub struct SourceSnapshotV1 { /* request/count/bytes/mount digest */ }
pub struct VerifiedSourceSnapshot { /* private snapshot + guard */ }
pub struct MountedSourceSnapshot { /* private verified snapshot + root */ }

pub trait SourceSnapshotProvider: Send + Sync {
    fn seal(
        &self,
        request: &SourceSnapshotRequestV1,
        limits: SourceSnapshotLimits,
    ) -> Result<VerifiedSourceSnapshot, InfraError>;
}

pub trait Sandbox: Send + Sync {
    fn execute(
        &self,
        invocation: &InvocationIdentityV1,
        source: &MountedSourceSnapshot,
        guard: SandboxExecutionGuardV1,
    ) -> Result<GuardedProcessObservationV1, InfraError>;
}

pub fn read_regular_file_bounded(
    root: &Path,
    relative: &RelativePathV1,
    limit: ByteLimit,
) -> Result<BoundedFileBytes, InfraError>;

pub fn stage_no_replace(
    path: &Path,
    bytes: &[u8],
    limit: ByteLimit,
) -> Result<(), InfraError>;

pub struct ResourcePolicyV1 { /* control/child CPU/RSS/output/queue limits */ }
pub struct ResourceClaimV1 { /* child and queue claim */ }
pub struct BoundedQueue<T> { /* FIFO with a fixed capacity */ }
```

`RelativePathV1` rejects empty, absolute, parent, empty, backslash, NUL, and
non-UTF-8 components. Reads inspect every component with `symlink_metadata`,
open the final regular file with the platform no-follow primitive, and read at
most `limit + 1` bytes so an over-limit file returns `InfraError::Quota`
without truncating into a valid value. Platforms without a reviewed no-follow
primitive fail closed with `InfraError::Guard`. No-replace staging uses
exclusive creation, bounded bytes, write completion, and `sync_all`; an
existing path can never be replaced.

`VerifiedSourceSnapshot`, `MountedSourceSnapshot`,
`SandboxExecutionGuardV1`, and `GuardedProcessObservationV1` have private
construction. The provider and sandbox traits are blocking and `Send + Sync`,
but only the protected runner can mint the verified values. This packet does
not expose a filesystem mount, process callback, scheduler worker, or authority
commit capability.

`ResourcePolicyV1` requires positive control-plane and child CPU/RSS/output
ceilings, child concurrency, and queue capacity. `ResourceClaimV1` is admitted
only when every dimension fits. `BoundedQueue<T>` is FIFO and rejects overflow
as a quota infrastructure error; it is a primitive, not a scheduler.

## Proof

```text
cargo check -p tsc-rs-ci-runner --lib
cargo test -p tsc-rs-ci-runner --test snapshot_resource
cargo test -p tsc-rs-ci-runner --lib
node .github/ci/slice-readiness.mjs --check fci-3c
```

Fixtures cover traversal/symlink rejection, bounded regular-file reads,
no-replace staging, positive resource ceilings, claim admission, FIFO queue
overflow, and the absence of async/live cache entries. Later packets add the
source inventory, action plan, and authority publication consumers.
