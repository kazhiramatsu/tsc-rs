# FCI-5c.1: H2.5g membership shadow

Status: **ready (FCI-5c.1 v1; non-authoritative, membership only)**.

This packet starts the Functional-CI migration without smuggling a workspace
read or a candidate process into the FCI-5b transport probe. It verifies the
fixed H2.5g plan against the exact qualified source and emits one deterministic
membership report. The legacy H2.5g qualification, inventory, acceptance,
owner-control, and hosted commands remain the only authority.

## Why execution is a separate packet

FCI-5b accepts an invocation identity and returns a bounded observation frame,
but it does not provide the candidate with source bytes, a source snapshot
capability, a case fixture, or a sandbox mount. Reading the checkout from the
candidate would make the result depend on ambient state and would violate the
functional-CI input contract. Therefore this packet does **not** claim a Rust
H2 observation, a cache hit, a root, or an acceptance result. The two-repetition
semantic execution is a later packet after the source-snapshot and runner
boundaries have been frozen.

## Pure function

```text
buildMembershipShadow(planBytes, qualificationBytes) -> canonicalReportBytes
```

Both arguments are immutable byte sequences. The function performs no I/O,
process spawn, environment read, clock read, path discovery, or global-state
mutation. It rejects malformed JSON, stale qualification bytes, a changed
profile/suite/denominator, non-contiguous shards, duplicate case ids, unknown
dispositions, or changed disposition counts.

The report contains the exact 9,027 denominator, `8511 admitted`, `6 H2.8a`
deferred, `510 H2.9` deferred, the source and plan digests, source-order case
id digests, and four fixed shard ranges. The plan's pre-existing membership and
shard digests are retained as bound evidence; the report also exposes the
recomputed source-order SHA-256 values so the execution packet can replace
the legacy opaque projection with a typed canonical membership object. The
report is explicitly `authoritative: false` and
`status: "shadow-membership-only"`.

Canonical object keys are sorted by UTF-8 bytes and arrays retain the qualified
source order. Running the function twice over the same bytes must return
identical bytes. No worker count, completion order, temporary path, PID, or
timestamp can appear in the result.

## Files and dependency boundary

- `.github/ci/fci-h2-5g-membership.mjs` owns the pure function and CLI wrapper.
- `.github/ci/fci-h2-5g-membership.test.mjs` owns deterministic and negative
  fixtures.
- `.github/ci/plans/h2-5g.v1.json` and
  `ratchets/h2-5g-qualification.v1.json` remain immutable inputs.

The packet does not change `ci-core`, the protocol wire format, the candidate
harness, `xtask`, production/compiler code, workflows, or any H2 authority
artifact. It does not add a cache, CAS, outcome manifest, projection,
capability, scheduler, or adapter semantic callback.

## Proof

```text
node --test .github/ci/fci-h2-5g-membership.test.mjs
node .github/ci/fci-h2-5g-membership.mjs --repeat-check
node .github/ci/slice-readiness.mjs --check fci-5c.1
```

The next execution packet must add a typed source-snapshot input and an
effect-owned runner entry before it may perform two isolated H2 observations.
Until that packet closes, the membership report is planning evidence only and
cannot alter H2.5g qualification or CI activation.
