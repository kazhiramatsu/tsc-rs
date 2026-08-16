# FCI-2a: blocking runner and error boundary

Status: **ready (FCI-2a v1; non-authoritative shadow)**.

This packet creates the private `ci-runner` package and only its closed
infrastructure-error vocabulary. It establishes the effect boundary needed by
the functional-CI design without introducing an executor or an effect
operation. No worker, source snapshot, process invocation, sandbox, resource
policy, staging, publication, CAS, cache, H2 type, or live evaluation entry is
authorized here.

## Base and allowed paths

- Trusted base: the closed FCI-1c commit recorded in
  `ratchets/fci-readiness/fci-2a.v1.json`.
- Allowed source paths: `Cargo.toml`, `Cargo.lock`, the `ci-runner` manifest,
  `src/lib.rs`, `src/error.rs`, its boundary tests, this packet, the slice
  index, and its readiness envelope.
- Forbidden: compiler, emitter, oracle, xtask, workflow, H2 profile,
  `ci-core` edits, and every future runner operation or authority surface.

## Frozen API

```rust
pub enum EffectPhase {
    Acquire,
    Read,
    Spawn,
    Execute,
    Join,
    Commit,
}

pub enum IoKind { /* payload-free std::io::ErrorKind projection */ }

pub enum RunCancellation {
    UserRequested,
    ProviderRequested,
    DeadlineExpired,
}

pub enum InfraErrorFamily {
    Io,
    Transport,
    Spawn,
    Signal,
    Timeout,
    Cancelled,
    OutOfMemory,
    Panic,
    Quota,
    Guard,
    Race,
    Durability,
}

pub enum InfraError {
    Io { phase: EffectPhase, kind: IoKind },
    Transport { phase: EffectPhase },
    Spawn { phase: EffectPhase },
    Signal { phase: EffectPhase },
    Timeout { phase: EffectPhase },
    Cancelled { phase: EffectPhase, reason: RunCancellation },
    OutOfMemory { phase: EffectPhase },
    Panic { phase: EffectPhase },
    Quota { phase: EffectPhase },
    Guard { phase: EffectPhase },
    Race { phase: EffectPhase },
    Durability { phase: EffectPhase },
}

impl InfraError {
    pub fn from_io(phase: EffectPhase, error: std::io::Error) -> Self;
    pub const fn from_panic(phase: EffectPhase) -> Self;
    pub const fn family(self) -> InfraErrorFamily;
    pub const fn phase(self) -> EffectPhase;
    pub const fn is_cancelled(self) -> bool;
}
```

All public values are `Send + Sync`, payload-free, and closed. I/O text,
platform payloads, panic payloads, timestamps, process ids, and retry data are
not semantic fields; later execution receipts may retain them separately.
`InfraError` implements `std::error::Error` only as a nonsemantic diagnostic
projection. It cannot be converted into a model rejection, a cache miss, a
successful observation, or a publication capability.

The public surface is synchronous and blocking. There is no async trait,
runtime handle, hidden executor, cancellation singleton, `RunContext`, worker,
snapshot, sandbox, cache, publication, or runner entry. Host code will pass an
explicit cancellation value at later safe points; this packet does not define
those safe points.

## Proof

```text
cargo test -p tsc-rs-ci-runner --test contracts error_boundary
cargo tree -p tsc-rs-ci-runner --edges normal,build,dev
node .github/ci/slice-readiness.mjs --check fci-2a
```

The tests cover every closed family, phase preservation, payload-free I/O and
panic conversion, all explicit cancellation reasons, `Send + Sync`, and the
absence of async/future/effect placeholders. The dependency tree must contain
only `tsc-rs-ci-runner` and `tsc-rs-ci-core`; no repository, compiler, oracle,
or `xtask` package may enter the runner closure.
