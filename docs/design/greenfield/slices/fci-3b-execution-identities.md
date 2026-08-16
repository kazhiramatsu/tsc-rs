# FCI-3b: execution, tool, reuse, and sandbox identities

Status: **ready (FCI-3b v1; non-authoritative shadow)**.

This packet adds generic, pure identity values used to describe an action's
execution inputs and reuse policy. It does not read the filesystem, spawn a
process, expose secrets, mount a snapshot, or declare the `Sandbox` trait.
Source snapshots, paths, guards, scheduling, resources, and effectful staging
remain FCI-3c or later.

## Base and allowed paths

- Trusted base: the closed FCI-3a commit recorded in
  `ratchets/fci-readiness/fci-3b.v1.json`.
- Allowed source paths: `Cargo.lock`, `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/identity.rs`, its identity tests, this packet, the slice
  index, and its readiness envelope.
- Forbidden: `ci-runner` effects, `Sandbox`/snapshot traits, filesystem/path
  access, resource scheduling, CAS/cache/publication, adapters, compiler,
  oracle, xtask, workflow, and H2 code.

## Frozen value API

```rust
pub struct ExecutionPlatformV1 { /* generic platform/capability tokens */ }
pub struct ToolRefV1 { /* id/role/artifact/platform */ }
pub struct ToolchainSetV1 { /* strict ordered ToolRefV1 values */ }
pub struct BuildComponentV1 { /* id/schema/digest */ }
pub struct BuildComponentSetV1 { /* strict ordered components */ }

pub struct PublicEnvironmentEntryV1 { /* key/value bytes */ }
pub struct SecretFreeEnvironmentV1 { /* strict ordered entries */ }

pub enum ReuseScopeV1 {
    NonReusable,
    LocalReusable,
    SharedReusable { audience: EvidenceAudienceV1 },
}

pub struct DisclosureHistoryV1 { /* audience -> first event digest */ }
impl DisclosureHistoryV1 {
    pub fn merge_monotonic(
        prior: &Self,
        replacement: &Self,
    ) -> Result<Self, DisclosureError>;
}

pub struct SandboxCapabilitiesV1 { /* ABI/network/fs/output ceiling */ }
pub enum ProcessObservationStatusV1 { Exited, Signaled, TimedOut, Cancelled }
pub struct ProcessObservationV1 { /* status and output digests */ }

pub struct InvocationIdentityV1 {
    /* adapter/schema/implementation/action/argv/public env/platform/toolchain
       sandbox/builder/capture/classifier identities */
}
```

All identity tokens are fixed-width, generic, and opaque. Tool references and
build components require strict `Ord` order, rejecting duplicates. Platform
identity contains OS/architecture/target/runtime/filesystem/path/sandbox/kernel
tokens and an explicit platform-independent bit; it never contains an opaque
runner-image label. Tool references bind a generic role, installation/content
digest, and platform identity. `InvocationIdentityV1` binds normalized argv and
working-directory bytes, the secret-free environment, action key, platform,
toolchain set, sandbox capability record, and builder/capture/classifier
implementation ids.

`SecretFreeEnvironmentV1` accepts an adapter-owned forbidden-key set and fails
closed on a match, empty key, duplicate, or unsorted entry. No secret-bearing
credential type exists in `ci-core`; transport credentials remain a later
runner/provider concern. A genuinely secret-dependent action must therefore
stay outside a reusable identity until a later effect policy explicitly marks
it non-reusable.

`DisclosureHistoryV1::merge_monotonic` preserves every prior audience and its
first publication-event digest, rejects shrinkage or first-event replacement,
and allows only new sorted audience entries. `ReuseScopeV1` is current policy
and is deliberately separate from that historical union.

Sandbox values are descriptive only: capabilities carry an ABI token, network
and filesystem mode, and a positive output ceiling; observations carry typed
exit/signal/timeout/cancellation status and output digests. There is no guard
constructor, mounted source, `Sandbox` trait, process callback, or publication
authority in this packet.

## Proof

```text
cargo test -p tsc-rs-ci-core --test contracts identity
node .github/ci/slice-readiness.mjs --check fci-3b
```

Fixtures cover strict tool/build ordering, secret exclusion, platform and
toolchain binding, disclosure monotonicity and first-event preservation,
typed sandbox observations, invocation identity fields, positive limits, and a
negative generic-literal audit. No generic source contains a repository,
compiler, or H2 noun.
