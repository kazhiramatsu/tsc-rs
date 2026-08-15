# FCI-5b: miss-only candidate harness

Status: **ready (FCI-5b v1; non-authoritative shadow)**.

This packet adds the process boundary for one candidate action. The harness is
an untrusted, miss-only executable: it receives one canonical
`ActionInvocationV1` frame and returns one bounded canonical
`ObservationEnvelopeV1` frame. It is not a verifier, cache, registry, root
aggregator, or authority process. H2.5g remains the only qualification
authority.

## Base and allowed paths

- Trusted base: the closed FCI-5a commit recorded in
  `ratchets/fci-readiness/fci-5b.v1.json`.
- Allowed source paths: workspace membership/dependency aliases, the
  protocol's typed strict decoders, the new candidate harness package and
  process tests, this packet, the slice index, and its readiness envelope.
- Forbidden: control/runtime changes, cache/CAS/publication/authority code,
  workflows, H2.5g artifacts and commands, and any semantic action
  registration.

## Frozen process contract

```text
stdin  = u32-be length || canonical ActionInvocationV1 bytes
stdout = u32-be length || canonical ObservationEnvelopeV1 bytes
```

The length is non-zero and bounded before allocation. Exactly one request and
one response are accepted; trailing input, malformed canonical JSON, unknown or
missing fields, invalid hex/identities, and output over the invocation limit
are failures. The child never emits logs on stdout. Error text is diagnostic
receipt data only and is not part of a semantic observation.

The harness has one deterministic transport probe until FCI-5c registers the
real H2 action model. It calls the existing checker entry from the candidate
assembly (and links the compiler crate as a candidate dependency) only to prove
that the process boundary is real. Its observation is a canonical,
version-tagged `transport-probe` record with the stable diagnostic count; no
H2 pass, cache hit, root, or capability is claimed.

No path, environment variable, clock, process id, network handle, cache
handle, control callback, or authority object is an input to the probe. The
absence of a source-path surface makes sandbox traversal unrepresentable at
this stage; later action packets must add a separately reviewed mounted-source
capability rather than smuggling a path into this wire format.

Child lifecycle failures remain infrastructure failures in the runner's
closed family (`spawn`, `signal`, `timeout`, `transport`, `quota`, or `guard`);
the harness never turns them into a semantic observation. This packet tests
malformed/truncated/over-limit frames, deterministic replay, non-zero exits,
and ambient-environment invariance. Signal/timeout classification is owned by
the already-frozen runner error vocabulary and is not duplicated here.

## Frozen API and dependency boundary

```text
crates/ci-adapter-tsc-rs-protocol/ -> ci-core only
crates/ci-harness-tsc-rs/          -> protocol + candidate production/compiler
```

The harness package is `tsc-rs-ci-harness` and its binary is
`tsc-rs-ci-harness`. It may link `tsc-harness` and `tsc-compiler`, but neither
the control package nor any generic runner/cache/authority package may link
back to it. The protected control plane can therefore complete a warm lookup
without compiling or spawning this candidate executable.

The protocol adds only strict typed decoders for the already frozen 5a
canonical objects. Decoding reuses `ci-core`'s bounded canonical parser and
reconstructs every typed value through its checked constructor; it does not
add callbacks, stringly action dispatch, or authority-bearing constructors.

## Proof

```text
cargo xtask test ci-adapter-tsc-rs-protocol
cargo xtask test ci-harness-tsc-rs
cargo xtask test ci-adapter-tsc-rs-control
cargo tree -p tsc-rs-ci-harness --edges normal,build,dev
cargo tree -p tsc-rs-ci-adapter-control --edges normal,build,dev
node .github/ci/slice-readiness.mjs --check fci-5b
```

Fixtures cover canonical replay, malformed and truncated frames, exact output
limits, trailing input, non-zero child failure, and changing the ambient
environment without changing output. Source/dependency audits assert that the
control closure contains no harness/production edge and that the harness has
no control/cache/authority edge. The tests do not modify or qualify H2.5g.
