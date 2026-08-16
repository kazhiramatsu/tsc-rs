# FCI-5c.1b: H2.5g observation shadow

Status: **design; PAUSED by the 2026-08-17 post-merge roadmap review
(Option A, emitter-first)**. The former unblock conditions are satisfied
and recorded — FCI-3c and FCI-5b are `closed` with green proofs and the
[post-H1 schedule](../post-h1-completion-slices.md#12-h25g-legacy-closing-protocol)
records the H2.5g validation/merge lineage — but this packet is not
`ready` and authorizes no production edit while the Functional-CI tail is
paused; an explicit post-H2.9 framework review may resume it. The packet
body below is retained verbatim for that review. Legacy H2.5g qualification,
inventory, acceptance, owner-control, and hosted commands remain the only
authority; this packet produces shadow evidence only and can mint no
qualification, acceptance, cache, root, or capability.

## Base, predecessors, and allowed paths

- Trusted base: the commit recorded in
  `ratchets/fci-readiness/fci-5c.1b.v1.json`.
- Predecessor receipts: `fci-5c.1` (membership shadow), `fci-3c`
  (source/sandbox/resource primitives, closed), and `fci-5b` (candidate
  process boundary, closed), recorded in the same envelope.
- Allowed paths: exactly the envelope's `allowedPaths` — workspace manifests,
  the `ci-runner` snapshot/sandbox files and their tests, the tsc-rs
  protocol/control adapter sources and tests, the candidate harness and its
  tests, one report-only `xtask` driver module plus its registration in
  `crates/xtask/src/main.rs`, this packet, the slice index, the architecture
  status row, the observation-summary ratchet, and the readiness envelope.
- Forbidden: production compiler/checker/emitter/oracle edits, `ci-core` and
  `ci-testkit` changes, workflows, H2.5g artifacts/schemas, CAS/cache/
  publication/authority surfaces, and any wire-format change to the frozen
  FCI-5a/5b protocol objects. A missing production seam is a fail-closed
  packet amendment, never an in-slice production edit.

## Boundary that must exist first (satisfied)

The FCI-5b child receives only an `ActionInvocationV1` identity and returns an
opaque bounded probe. It has no source bytes or mounted capability. This
packet must not add a path, checkout, environment variable, or current
working directory to that wire object. The only legal acquisition path is:

```text
immutable H2 case input bytes
  -> SourceSnapshotRequestV1
  -> SourceSnapshotProvider::seal
  -> VerifiedSourceSnapshot + generation guard
  -> MountedSourceSnapshot
  -> Sandbox::execute(InvocationIdentityV1, MountedSourceSnapshot, guard)
  -> bounded canonical child observation
```

The runner owns this effect sequence. The adapter supplies a typed case
specification and expected repetition policy; it does not open files, spawn a
process, select a worker, or inspect ambient state. A sandbox implementation
may choose a mount, descriptor, or platform-specific isolation mechanism, but
the chosen mechanism is behind the already typed `Sandbox` capability and is
not represented as a semantic input path.

The already frozen FCI-5a wire objects carry everything this packet needs:
`ActionInvocationV1` binds action/schema/implementation identities, the case
`input` digest, the sealed `source_snapshot` digest, the repetition ordinal
(`0` or `1`), and `max_output_bytes`. No new invocation field is added.

## Frozen pure input and result

The adapter-owned case specification is the tuple:

```text
case_id
action_key
schema_id
implementation_id
canonical_input_digest
source_snapshot_request
expected_repetitions = 2
max_output_bytes
```

The runner derives one `InvocationIdentityV1` from that tuple and the sealed
source snapshot. Each repetition gets a distinct invocation id for lifecycle
accounting, but invocation id, PID, worker number, completion order, mount
spelling, clock, and temporary directory are excluded from the semantic
observation. The child receives one bounded protocol frame and must return one
bounded canonical observation frame. A child error, timeout, signal, quota,
guard mutation, or snapshot failure is `InfraError`; it is never converted to a
semantic rejection or cache miss.

The pure adapter step is:

```text
verify_two(case_spec, observation_0, observation_1)
    -> CompleteCaseObservation | SemanticMismatch
```

Both observations must bind the same action/schema/implementation/input and
the same source-snapshot digest. The bytes must be canonical and exactly equal
for the deterministic shadow. The membership report from FCI-5c.1 supplies
the admitted case ids and the two-repetition policy; deferred rows are never
invoked. No root, outcome manifest, cache candidate, projection, or authority
capability is produced by this packet.

## Frozen Rust surface

`crates/ci-runner` (generic; no H2/tsc noun):

```rust
pub struct DirectorySnapshotProvider { /* root + ordered entry list */ }

impl DirectorySnapshotProvider {
    pub fn try_new(
        root: std::path::PathBuf,
        entries: Vec<RelativePathV1>,
    ) -> Result<Self, InfraError>;          // sorted, duplicate-free entries
    pub fn mount(
        &self,
        verified: &VerifiedSourceSnapshot,
    ) -> Result<MountedSourceSnapshot, InfraError>;
}

impl SourceSnapshotProvider for DirectorySnapshotProvider { /* seal */ }

pub struct ProcessSandboxV1 { /* executable, request frame, staging root,
                                 byte and second ceilings */ }

impl ProcessSandboxV1 {
    pub fn try_new(
        executable: std::path::PathBuf,
        request_frame: Vec<u8>,
        staging_root: std::path::PathBuf,
        max_output_bytes: ByteLimit,
        timeout_seconds: u64,
    ) -> Result<Self, InfraError>;
    pub fn staged_observation_frame(
        &self,
        observation: &GuardedProcessObservationV1,
    ) -> Result<BoundedFileBytes, InfraError>; // reread + digest recheck
}

impl Sandbox for ProcessSandboxV1 { /* execute */ }
```

- `seal` re-reads every entry with `read_regular_file_bounded`, enforces
  `SourceSnapshotLimits`, verifies the request's `entries` digest against the
  ordered entry-list digest, and computes the mount digest over ordered
  `(path, bytes)` pairs; the guard digest is that mount digest.
- `execute` first verifies identity/frame coherence (the configured frame's
  action/schema/implementation equal the passed identity's), then recomputes
  the mounted digest and fails `InfraError::Guard { phase: Execute }` on any
  byte drift before spawning. The child runs with working directory equal to
  the mount root, the identity's `SecretFreeEnvironmentV1` as its entire
  environment, one request frame on stdin, and a bounded stdout read. Stdout
  frame bytes are staged with `stage_no_replace` under the invocation-private
  staging root; the returned `ProcessObservationV1` carries status and
  stdout/stderr digests only. Spawn/signal/timeout/oversize failures use the
  closed `InfraError` families `Spawn`, `Signal`, `Timeout`, and `Quota`.

`crates/ci-adapter-tsc-rs-protocol` (wire additions; no invocation change):

```rust
pub struct CaseSpecV1 { /* case_id, action, adapter_schema, implementation,
                           canonical_input, source_revision, source_provider,
                           source_entries, expected_repetitions == 2,
                           max_output_bytes */ }
pub struct CaseOutputV1 { /* validated relative path string, sha256, length */ }
pub struct CaseObservationPayloadV1 { /* case_id, source_mount digest,
                                         diagnostic count + digest,
                                         ordered outputs */ }
```

Both carry `CanonicalEncode` plus a strict bounded `decode_canonical` through
the existing checked constructors; unknown fields, duplicates, wrong widths,
noncanonical bytes, and `expected_repetitions != 2` fail closed. The case
observation schema id is the constant `h2_case_observation_schema()`, defined
as the first 16 bytes of the SHA-256 of the ASCII string
`tsc-rs.h2.case-observation.v1`; the FCI-5b transport probe keeps its
existing schema value and behavior.

`crates/ci-harness-tsc-rs`: when the decoded invocation's schema equals
`h2_case_observation_schema()`, the child reads the case plan and fixture
bytes from its working directory (the mount), verifies their recomputed
digests against the invocation's `input` and `source_snapshot` digests, runs
the already-public candidate compile entry against in-memory sources with a
memory sink, and returns one `CaseObservationPayloadV1` inside the existing
`ObservationEnvelopeV1`. Any other schema keeps the FCI-5b transport probe.
The child writes no output file and reads nothing outside its working
directory.

`crates/ci-adapter-tsc-rs-control` (pure; no process/spawn/link change):

```rust
pub struct CompleteCaseObservationV1 { /* spec + one canonical payload +
                                          both repetition envelope digests */ }
pub struct CaseObservationMismatchV1 { /* spec + ordered repetition payload
                                          digests + first differing field */ }
pub fn verify_two(
    spec: &CaseSpecV1,
    first: &ObservationEnvelopeV1,
    second: &ObservationEnvelopeV1,
) -> Result<CompleteCaseObservationV1, CaseObservationMismatchV1>;
```

`crates/xtask`: one report-only driver module `fci_observation_shadow.rs`
registered as `cargo xtask fci-h2-observation-shadow [--fixture | --full]`.
It composes the membership report inputs, provider, sandbox, harness
executable, and `verify_two`; it owns no semantics, changes no legacy
command, and its `--full` run writes the summary ratchet below.

Registering the subcommand edits `crates/xtask/src/main.rs`, whose raw bytes
are content-addressed by both the hosted CI policy
(`.github/ci/qualification-policy.v2.json`) and the H2.5g profile's
runtime-input pins. This packet therefore performs that pin refresh as one
explicit reviewed transition: update the policy's recorded source hash and
re-mint `ratchets/h2-5g-profile.v1.json` with its checked-in generator on the
approved profile host, exactly as the earlier in-branch profile refresh did.
The immutable H2.5g qualification and owner-control artifacts are not
regenerated, reinterpreted, or rewritten by this packet.

## Required controls

- Run the same case twice in isolated child lifetimes; changing completion
  order must not change report bytes.
- Change the ambient environment, process id, current directory spelling, and
  worker count while keeping the sealed snapshot and canonical case bytes
  fixed; the semantic observation must remain byte-identical.
- Mutate one source byte after sealing; the generation guard must fail before
  the child starts.
- Attempt `..`, absolute, backslash, symlink, and outside-mount reads; the
  sandbox must return an infrastructure error.
- Truncate, reorder, duplicate, or add fields to the child frame; strict
  protocol decoding must fail closed.
- Return different observations for repetitions; report a deterministic
  semantic mismatch without cancelling sibling cases.
- Verify that a warm control decision does not compile or spawn the candidate
  harness; the first spawn is owned by a cold miss only.

## Implementation order

1. Land `DirectorySnapshotProvider` and `ProcessSandboxV1` in `ci-runner`
   with their seal/mount/guard/spawn/staging contracts and contract tests.
2. Add `CaseSpecV1`, `CaseOutputV1`, `CaseObservationPayloadV1`, and the
   schema constant to the protocol crate with golden and adversarial decode
   fixtures.
3. Add the harness case action behind the schema constant; keep the
   transport probe byte-identical for its existing schema.
4. Add `verify_two` and the two typed results to the control adapter with
   deterministic mismatch fixtures.
5. Add the `xtask` driver: `--fixture` runs the checked-in two-file fixture
   case set with exactly two isolated repetitions per case; `--full` runs all
   8,511 admitted cases from the FCI-5c.1 membership report, two isolated
   repetitions each, and writes `ratchets/fci-5c1b-observation.v1.json`
   containing schema, membership digest, per-shard observation-set digests,
   the global observation-set digest, counts, `authoritative: false`, and
   `status: "shadow-observation-only"`. The full run is legal only because
   the roadmap review records the final H2.5g validation reference.

## Proof

```text
cargo xtask test ci-runner
cargo xtask test ci-adapter-tsc-rs-protocol
cargo xtask test ci-adapter-tsc-rs-control
cargo xtask test ci-harness-tsc-rs
cargo xtask fci-h2-observation-shadow --fixture
cargo xtask fci-h2-observation-shadow --full
node .github/ci/slice-readiness.mjs --check fci-5c.1b
```

The `--fixture` line must end
`FCI-5c.1b fixture shadow: cases=<n> observations=<2n> mismatches=0
authoritative=false` for the checked-in fixture count. The `--full` line must
be exactly
`FCI-5c.1b observation shadow: cases=8511 observations=17022 mismatches=0
membership=3c54352fbb44d7ff29dc9bedd8024cf03ea2c9c3ab4a80bbe73d445db32ca147
authoritative=false`, where the membership digest equals the FCI-5c.1
report's `case_id_sha256`. A deterministic mismatch fails the proof and
blocks FCI-5c.2; it cannot alter H2.5g authority.

## Explicit non-goals

This packet does not add CAS/cache lookup, partial reuse, outcome manifests,
verified roots, publication, a live scheduler, a generic H2 branch in
`ci-core`, or a direct dependency from protected control to production. It
does not alter the H2.5g legacy commands, counts, profile, owner controls,
hosted `ts-tests` boundary, or qualification authority.
