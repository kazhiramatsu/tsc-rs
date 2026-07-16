# M8 scope and readiness contract

This is the executable contract for entering M8. The normative end
state remains [definition-of-done.md](definition-of-done.md); this
page defines how its supported-scope and M8-start clauses are
measured. M5-M7 implementation is a prerequisite, not part of this
work.

## Two views of the same corpus

Every conformance run reports both views. Neither is optional.

1. **All-corpus visibility** is the versioned oracle-input universe
   (5,908 fixtures at the adopted baseline, growable only by A1's
   append-only transition). It keeps the T0 ratchet, top FN codes, and the
   absolute `FP=0` gate. Out-of-scope work remains visible here.
2. **Supported scope** removes only reviewed oracle diagnostics in
   `tsrs2/m8-scope.json`. M8's T1-T4 completion target uses this
   denominator.

Scope dispositions are exact diagnostic identities in the A2
schema-2 form:
`fixture + matrix_key + pass + file + start + length + code +
category + message-chain hash + related-information hash +
occurrence`; line and column ride along as review-facing redundant
fields verified against `start`, never as the identity. There are no
fixture, directory, code, or glob exclusions, and schema-1 T0-key
dispositions are rejected with a migration message. Because the
identity distinguishes records that share a T0 key, one
host-resolution or JSDoc diagnostic cannot hide another diagnostic
in the same program — not even its duplicate-bucket neighbor.
Syntactic diagnostics can never be excluded. Message-chain and
related-information hashes use the schema-tagged canonical UTF-8
encoding defined by convergence-plan A2 (fixed field order, observable
array order, lowercase SHA-256); alternate Node/Rust serializations are
not accepted as equivalent identities.

The only accepted reasons are:

- `host-resolution`: behavior requiring node_modules/package.json,
  paths/baseUrl host lookup, project references, or redirects;
- `jsdoc-semantics`: checking whose result depends on a JSDoc type;
- `emit-dependent`: a diagnostic whose observability depends on the
  deliberately absent emitter.

Every entry needs non-empty evidence. The manifest starts `draft`.
A band can be pinned earlier without freezing the manifest: an A2
band-freeze record (the band's enumerated pinned identity set inside
`m8-scope.json`, anchored to its adjudication commit) makes that
band immutable while status stays `draft` — the `2xxx` band pins at
the phase-9 sweep (convergence plan §4 row 9; the A2 section defines
the subset / history-anchor / tombstone rules). At M7 close, classify
the remaining known out-of-scope FNs and land the reviewed exact set
while status is still `draft`. A follow-up change records the A2
global-freeze identity set against that adjudication commit and changes
status to `frozen`. The audit re-reads that commit, requires every live
exclusion to belong to the anchored global set, and re-verifies every
early band record. Thus a post-freeze addition/edit in an otherwise
unpinned non-2XXX band fails; count/hash refresh or status downgrade
cannot hide it. Required PR CI also compares the global-freeze record to
the trusted base, so an add-and-reanchor pair of branch commits fails.
If tsrs later emits an excluded diagnostic at T0,
conformance reports it as `resolved-t0`; delete the disposition
immediately so T1-T4 begin grading it. Every post-global-freeze deletion,
and every earlier pinned-band deletion, carries the A1 membership
tombstone; a duplicate T0 bucket additionally must be
multiplicity-complete. Pinned identity sets never change.

`cargo xtask conformance` prints both metric sets and writes both to
the mismatch JSON. The all-corpus FP gate and T0 ratchet are
unchanged.

## All-band inventory and dependency closure

Run:

```sh
cargo xtask codegen band-inventory --by-function --band all
cargo xtask codegen band-inventory --by-function --band all --check
```

The generated `tsrs2/m8-emitter-inventory.json` is pinned to the
SHA-256 of the vendored `_tsc.js`. It inventories every direct
`Diagnostics.*` use except `.code` membership reads and computes a
transitive, deliberately conservative identifier/property-call
dependency closure. Schema 2 keys every named and anonymous
function-like declaration by lexical-owner declaration path, kind,
name-or-anonymous, start/end, and source-slice SHA-256. Duplicate bundle
declarations with the same function name are distinct identities;
anonymous callbacks are not attributed to their nearest named owner.
The tsc function name remains a ledger alias only. A `tsc-port` match is
automatic only when its `tsc-span` and `tsc-hash` select the exact
declaration.

`m8-emitter-dispositions.json` is the tsc-to-Rust converse of the
Rust function ledger:

- the file pins the exact generated inventory SHA-256, so generator
  changes require a reviewed refresh even when `_tsc.js` is unchanged;
- a matching `tsc-port` span/hash is classified automatically; a bare
  same-name match is not;
- remaining closure functions require exact `deferred` or
  `not-applicable` entries with evidence;
- no missing or stale entry is accepted when the file is `frozen`.

Identifier calls resolve lexically to declarations. Property-call
closure remains an over-approximation, so a dynamic-dispatch false
dependency may be `not-applicable`, but every candidate declaration
keeps a separate edge and disposition. Schema-1 name-collapsed inventory
and dispositions may exist only as pre-D2 draft migration input; they
cannot be frozen or satisfy readiness.

## Runtime, fuzzer, and performance evidence

`tsrs2/m8-evidence.json` holds producer configuration — declared
commands, approved runner ids, and reviewed ceilings — not editable
`ready` booleans or copied observations. Producers write ephemeral
artifacts under `target/`; readiness reads raw observations, recomputes
each summary, and requires the artifact's producer-input fingerprint to
equal the current executable/source/toolchain/vendor/corpus/oracle/scope/
inventory/policy inputs that apply. An ancestor commit alone is not
freshness. Relevant dirty paths and missing artifacts fail; a fresh CI
clone runs all producers before readiness in the same workspace.

- Runtime coverage pins the exact inventory SHA-256 and lists every
  declaration-level direct-emitter identity exactly once as
  executed or zero-hit-with-evidence and must have at least one executed
  emitter. Same-name declarations have independent counters. Unknown,
  duplicate, overlapping, name-collapsed, or evidence-free identities fail.
- The differential fuzzer must run a generator against both tsrs and
  the pinned oracle, compare every generated case, exercise its
  reducer and signature deduper, and name the CI command that keeps it
  running.
- Performance records wall time, maximum RSS, reviewed ceilings, and an
  approved reference-runner profile. The wall ceiling may not exceed 60
  seconds, and both observations themselves must be within their
  ceilings on that profile. Artifact paths are workspace-relative and
  may not escape the workspace.

The minimal differential loop is an **M8-start** prerequisite. M9
does not introduce the fuzzer; it hardens the already-running loop
to the versioned 14-window steady-state gate in convergence-plan B3.

## Machine gate

```sh
cargo xtask m8 readiness
cargo xtask m8 readiness --require-ready
```

The first command is a report and succeeds while work remains. The
second is the M7-close/M8-entry gate and fails unless all ten rows
are green:

1. M7 conformance gate (`T0 >= 63%`, `FP=0`, configured exact T1 ratchet,
   enforced by All-band conformance);
2. live T1-T3 shadow metrics;
3. globally identity-anchored, frozen, non-stale scope manifest;
4. zero undispositioned Rust checker functions;
5. fresh declaration-identity schema-2 all-band emitter inventory;
6. frozen and complete dependency dispositions;
7. complete runtime coverage evidence with the current producer-input
   fingerprint;
8. an actually running differential fuzzer with current smoke evidence;
9. current performance and RSS observations within the recorded ceilings
   on an approved reference-runner profile;
10. every M7-owned family complete in the A5 rollup — `families
    check` green and supported FN = 0 for each family's (code, pass)
    rows, recomputed from the same full-conformance observation after
    applying the globally frozen exact A2 scope to the immutable oracle
    and current tsrs multisets, then grouped by the frozen family map.
    A1 supplies the monotonic guard, not a substitute for the current
    supported projection. The map's base and any append-only
    universe-extension rows are identity-anchored to their adjudication
    commits (A5), so M7 ownership cannot shrink after the freeze to fake
    completion.
    Row 1's aggregate cannot substitute: 63% is reachable from the
    unused family alone, so a red family fails this row and is
    named.

Required regression test: a state meeting row 1 (`T0 >= 63%`,
`FP=0`) while any M7-owned family row is red fails
`--require-ready` and names the family. Row 10 also covers one wholly
excluded bucket and one duplicate bucket with only one excluded record;
the former contributes no supported denominator and the latter retains
its supported neighbor. A rollup computed from A1+map without the exact
scope/current conformance input is rejected as stale.

The JSON report is written to `target/m8/readiness.json`. The command
does not pretend that the current M4 branch is M8-ready; it makes the
remaining prerequisites explicit and mechanically testable.

## Escape end state

Recovery guards may remain a separately ratcheted `Unsupported`
class through M5-M7, but they are not a permanent exception to Done.
Before final completion they must move to deterministic recovery
values/control flow that do not use the `Unsupported` containment
channel. The final escape requirement remains `sites=0` and an empty
manifest.
