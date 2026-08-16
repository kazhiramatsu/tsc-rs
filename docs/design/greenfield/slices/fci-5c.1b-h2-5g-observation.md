# FCI-5c.1b: H2.5g observation shadow

Status: **design / blocked**. This packet is intentionally not runtime-ready.

It is the successor to [FCI-5c.1 membership shadow](fci-5c.1-h2-5g-membership.md).
It may become `ready` only after the source-snapshot and runner packets have
closed and the final H2.5g validation/merge reference has been recorded.

## Boundary that must exist first

The FCI-5b child currently receives only an `ActionInvocationV1` identity and
returns an opaque bounded probe. It has no source bytes or mounted capability.
This packet must not add a path, checkout, environment variable, or current
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

## Implementation order after unblock

1. Freeze the source-snapshot request/guard and runner entry signatures in the
   owning FCI-3c/FCI-7 packet; add no adapter-specific path escape.
2. Add the H2 case-spec and strict observation decoder in the H2 adapter packet.
3. Connect one runner invocation to the existing candidate process boundary.
4. Add the two-isolated-repetition collector and deterministic mismatch value.
5. Run focused 5c.1b controls on a small fixture, then the full 8,511 admitted
   cases only after the final H2.5g validation reference.

## Explicit non-goals

This packet does not add CAS/cache lookup, partial reuse, outcome manifests,
verified roots, publication, a live scheduler, a generic H2 branch in
`ci-core`, or a direct dependency from protected control to production. It
does not alter the H2.5g legacy commands, counts, profile, owner controls,
hosted `ts-tests` boundary, or qualification authority.
