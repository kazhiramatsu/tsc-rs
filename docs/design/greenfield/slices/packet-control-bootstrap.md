# Packet-control bootstrap for pre-closure Functional-CI shadows

Status: **bootstrap record**. This is the one-time control-plane packet that
installs and checks the versioned slice-readiness format. It is not a runtime
CI implementation, an H2.5g qualification record, or an authority publisher.

## Purpose

The Functional-CI architecture originally placed every implementation packet
after H2.5g closure. The current development cost of repeatedly rebuilding a
large H2 corpus requires an earlier, still fail-closed shadow path. This
bootstrap changes only the scheduling boundary: it permits the dependency
ordered FCI-1a through FCI-5b packets and the narrow FCI-5c.1 H2.5g inventory
profile to be prepared before closure. It never changes the legacy H2.5g
commands, denominator, disposition counts, hosted scope, owner-control scope,
or acceptance authority.

## Bootstrap authority

The bootstrap may authorize only the following transitions:

```text
design -> ready (packet-control schema/checker and tests)
design -> ready (FCI-1a ... FCI-5b, one packet at a time)
design -> ready (FCI-5c.1, only after FCI-5b closes)
```

It may not authorize FCI-5c.2, FCI-6 or later, any workflow/hosted/provider
change, any cache or outcome capability, or any H2.5g closure claim. A packet
must name its trusted base, exact paths, frozen symbols, proof commands, and
expected evidence. Missing, stale, or malformed readiness data is a hard
failure; the implementer cannot fill in an omitted field.

The checker is called by the existing protected qualification entry point. It
validates the packet body digest, the machine envelope, predecessor packet
receipt, allowed/forbidden path set, and the bootstrap transition. It does not
execute production code, inspect candidate-generated policy, or infer packet
semantics from a stage heading. Its output is a non-authoritative readiness
receipt used only to permit the next indexed development step.

## Initial allowed paths

The bootstrap implementation may touch only:

- `.github/ci/contracts/slice-readiness.v1.schema.json`;
- `.github/ci/slice-readiness.mjs` and its focused test;
- `ratchets/fci-packet-bootstrap.v1.json` and the per-packet readiness
  envelopes under `ratchets/fci-readiness/`;
- this packet, the slice index, the Functional-CI architecture, and the
  post-H1 schedule paragraphs that describe the order; and
- the first packet's explicitly listed files after that packet becomes
  `ready`.

Emitter, parser, checker, compiler, H2.5g profile/qualification/owner-control,
workflow, hosted-provider, and acceptance implementation files are forbidden
to the bootstrap. A bootstrap proof that sees any forbidden path fails closed.

## Required proof

The bootstrap closes only when the following are byte-stable and green:

```text
node .github/ci/slice-readiness.mjs --schema-check
node .github/ci/slice-readiness.mjs --test
node .github/ci/qualification.mjs check
```

The proof records the schema digest, checker digest, bootstrap record digest,
and the exact current base commit in `ratchets/fci-packet-bootstrap.v1.json`.
The record is not itself an H2.5g result and cannot be used as a cache key for
compiler observations.
